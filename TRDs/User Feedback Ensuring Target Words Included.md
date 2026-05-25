# TRD: User Feedback Ensuring Target Words Included

## Overview

Revamp the user chat feedback mechanism to check whether the user used the target learning words (difficult + known words from the SSR system prompt), and add an edit button so users can revise their message to include those words.

---

## PRD Requirements

1. **Feedback must check target word usage** — LLM evaluates whether the user used the target words from the SSR prompt in their response.
2. **Suggest corrections if target words are missing** — If the user didn't use target words, provide a suggested response that includes them.
3. **Word-by-word usage table** — Feedback format must include a table showing each target word and whether it was used.
4. **Rating rules for target words** — If the user uses zero target words, rating = 0. Otherwise, rating follows the LLM judgement (1–5).
5. **Edit button for user messages** — Empty input (no pre-fill), forces retyping to reinforce learning.
6. **Edit triggers full re-send** — Re-triggers LLM chat (new bot response) + re-evaluates feedback.
7. **Hide previous messages** — Old user message and bot response are hidden but expandable/collapsible.

---

## Technical Changes

### 1. Revamp Feedback Evaluation Prompt (Backend)

**File:** `src-tauri/src/lib.rs` — `db_evaluate_user_message` (line ~1721)

**Current behavior:** The LLM evaluates grammar and vocabulary generically. It has no knowledge of which target words the SSR system prompt asked the bot to use, nor does it check whether the user used those words.

**Change:** Pass the target words (difficult + known) into the evaluation command and include them in the prompt.

#### 1a. Add `target_words` parameter to `db_evaluate_user_message`

```rust
// New parameter
target_words: Vec<String>,  // e.g. ["食べる", "行く", "元気"]
```

#### 1b. Update the evaluation prompt

Add a new section to the prompt that lists the target words and instructs the LLM to:

- Check which target words the user actually used
- Produce a word-by-word usage table
- If target words are missing, suggest a revised response that includes them

New prompt section (appended after existing prompt body):

```
## Target Words Check

The learner was expected to use these target words in their response:
{target_words_list}

You MUST include a section:

### 🎯 Target Word Usage

| Status | Target Word | Used? | Notes |
|:---:|:---|:---:|:---|
| ✅/❌ | word | Yes/No | How it was used, or suggested usage |

If any target words were NOT used, add a section:

### 💡 Suggested Response with Target Words

Provide a revised version of the learner's message that naturally includes ALL the missing target words.
```

#### 1c. Score override: 0 if no target words used

After the LLM returns a score, check if any target words were actually used by the user. If none were used, override the score to 0.

**Implementation:** In `db_evaluate_user_message`, after getting the LLM response, tokenize the user's message and check against `target_words`:

```rust
// After LLM call, check if user used any target words
let user_words = tokenize_for_matching(&message_content);
let used_any = target_words.iter().any(|tw| {
    user_words.contains(&tw.to_lowercase())
});

let final_score = if !used_any && !target_words.is_empty() {
    0  // Override: no target words used → score = 0
} else {
    extract_score_from_feedback(&feedback)
};
```

For Japanese, use the existing Lindera tokenizer to get base forms for matching. For other languages, use simple lowercase word splitting.

Add a helper in `lib.rs`:

```rust
/// Tokenize message content into lowercase words for target-word matching.
/// For Japanese, uses Lindera to get base forms. For other languages, splits by whitespace.
fn tokenize_for_matching(content: &str) -> Vec<String> {
    // Simple whitespace + punctuation split for non-Japanese
    content
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|s| !s.is_empty())
        .collect()
}
```

Note: For Japanese, we need to use Lindera tokenization (base forms) for accurate matching. This requires access to the `FuriganaAnnotator` state. Pass it as an optional parameter or use a separate Tauri command. Given complexity, the simplest approach is to do the target-word check on the **frontend** where Lindera tokenization is already available (see Section 1d below).

#### 1d. Frontend score override (simpler approach)

Instead of tokenizing in Rust, check target word usage on the frontend where we already have Lindera tokenization:

**File:** `src/pages/Chat.tsx`

After feedback is received, check if the user used any target words and override the score:

```ts
// In the feedback .then() callback
.then(fb => {
  // Check if user used any target words
  const userUsedAnyTarget = checkTargetWordUsage(msg, targetWords)
  const finalScore = (!userUsedAnyTarget && targetWords.length > 0) ? 0 : fb.score
  setFeedbackMap(prev => {
    const next = new Map(prev)
    next.set(userId, { ...fb, messageId: userId, score: finalScore, loading: false })
    return next
  })
})
```

Add helper function:

```ts
/**
 * Check if the user's message contains any of the target words.
 * For Japanese, uses Lindera tokenization (base forms).
 * For other languages, uses simple word matching.
 */
function checkTargetWordUsage(message: string, targetWords: string[]): boolean {
  if (lang === 'ja') {
    // Use the already-tokenized word colors data if available
    // Fall back to simple includes check
    return targetWords.some(tw => message.toLowerCase().includes(tw.toLowerCase()))
  }
  const words = message.toLowerCase().split(/\s+/)
  return targetWords.some(tw => words.includes(tw.toLowerCase()))
}
```

#### 1e. Update the frontend call site

**File:** `src/pages/Chat.tsx` — `send()` function (line ~648)

Change to include the target words that were used in the SSR prompt for that message:
```ts
const targetWords = buildTargetWordsList()  // new helper, see below

invoke<UserFeedback>('db_evaluate_user_message', {
  apiHost, apiKey, model,
  messageId: userId,
  messageContent: msg,
  language: lang || '',
  level: lang === 'ja' ? jlptLevel : '',
  contextMessages: contextMsgs,
  targetWords,
})
```

#### 1f. Extract `buildTargetWordsList()` helper

**File:** `src/pages/Chat.tsx`

Extract the word selection logic from `buildSSRSystemPrompt()` into a reusable function:

```ts
function buildTargetWordsList(): string[] {
  const difficultArr = Array.from(difficult.keys())
  const knownArr = Array.from(known).filter(w => !difficult.has(w))
  const maxWords = getMaxDifficultWords()
  const maxKnown = getMaxKnownWords()
  const [selectedDifficult] = dequeueRotate(difficultArr, wordQueueIndex.current.difficult, maxWords)
  const [selectedKnown] = dequeueRotate(knownArr, wordQueueIndex.current.known, maxKnown)
  return [...selectedDifficult, ...selectedKnown]
}
```

Then `buildSSRSystemPrompt()` calls `buildTargetWordsList()` internally to avoid duplication.

**Important:** The target words passed to feedback must match the words used in the SSR prompt for the *same* message. Since `wordQueueIndex` advances after each call, we must capture the target words *before* `buildSSRSystemPrompt()` is called and pass the same list to both.

Refactor `send()`:
```ts
const targetWords = buildTargetWordsList()
const systemPrompt = buildSSRSystemPrompt(targetWords)  // pass in instead of recomputing
// ... later, pass targetWords to db_evaluate_user_message
```

---

### 2. Add Edit Button for User Messages (Frontend)

**File:** `src/pages/Chat.tsx`

#### 2a. Add edit state

```ts
const [editingMessageId, setEditingMessageId] = useState<number | null>(null)
const [editText, setEditText] = useState('')
```

#### 2b. Add edit button next to user messages

In the message rendering section (line ~1068), add an edit icon button for user messages:

```tsx
{m.role === 'user' && m.id && !editingMessageId && (
  <button
    onClick={() => { setEditingMessageId(m.id); setEditText('') }}  // Empty — no pre-fill
    className="mt-1 text-blue-300 hover:text-blue-100 transition-colors"
    title="Edit message"
  >
    <EditOutlined style={{ fontSize: 12 }} />
  </button>
)}
```

Import `EditOutlined` from `@ant-design/icons`.

#### 2c. Render edit mode

When `editingMessageId === m.id`, replace the message bubble content with an **empty** TextArea + "Edit" label + Send/Cancel buttons:

```tsx
{editingMessageId === m.id ? (
  <div className="w-full">
    <div className="flex items-center gap-1 mb-1">
      <span className="text-[10px] font-semibold text-blue-200 uppercase tracking-wider">Edit</span>
    </div>
    <TextArea
      value={editText}
      onChange={e => setEditText(e.target.value)}
      placeholder="Retype your message using target words..."
      autoSize={{ minRows: 1, maxRows: 4 }}
      className="rounded-xl bg-blue-400 text-white placeholder-blue-200"
    />
    <div className="flex gap-2 mt-1">
      <button
        onClick={() => handleEditSave(m)}
        disabled={!editText.trim()}
        className={`text-xs text-white px-3 py-1 rounded-full ${
          editText.trim() ? 'bg-blue-600 hover:bg-blue-700' : 'bg-blue-400/50 cursor-not-allowed'
        }`}
      >
        <SendOutlined style={{ fontSize: 10 }} /> Send
      </button>
      <button onClick={() => setEditingMessageId(null)} className="text-xs text-white bg-gray-500 px-3 py-1 rounded-full">
        Cancel
      </button>
    </div>
  </div>
) : m.content}
```

#### 2d. Implement `handleEditSave`

This function must:
1. Save the new message content to DB
2. Hide the old user message and old bot response (mark as superseded)
3. Re-trigger LLM chat (new bot response)
4. Re-evaluate feedback for the edited message

```ts
async function handleEditSave(m: Msg) {
  if (!m.id || !editText.trim()) return
  const newContent = editText.trim()

  // 1. Update message content in DB (also clears old feedback)
  await invoke('db_update_message_content', { messageId: m.id, content: newContent })

  // 2. Find the bot response that follows this user message and mark both as hidden
  const msgIndex = messages.findIndex(msg => msg.id === m.id)
  const botResponse = msgIndex >= 0 ? messages[msgIndex + 1] : null

  // Mark old user message + bot response as superseded (hidden but expandable)
  setSupersededMessages(prev => {
    const next = new Set(prev)
    next.add(m.id!)
    if (botResponse?.id) next.add(botResponse.id)
    return next
  })

  // 3. Add the edited user message as a new visible message
  const editedMsg: Msg = { id: null, role: 'user', content: newContent }
  setMessages(prev => [...prev, editedMsg])

  // Save edited message to DB as a new message
  const editedId = await invoke<number>('db_add_message_in_session', {
    sessionId, role: 'user', content: newContent
  }).catch(() => -1)

  if (editedId >= 0) {
    setMessages(prev => prev.map((msg, idx) =>
      idx === prev.length - 1 ? { ...msg, id: editedId } : msg
    ))
  }

  setEditingMessageId(null)

  // 4. Re-trigger LLM chat + feedback (same flow as send())
  setLoading(true)
  if (sessionId) setLLMPending(sessionId)

  const targetWords = buildTargetWordsList()
  const systemPrompt = buildSSRSystemPrompt(targetWords)

  // Build conversation history: all visible messages up to the edited one
  const visibleMessages = messages.filter(msg => !supersededMessages.has(msg.id!))
  const llmMessages = [
    { role: 'system' as const, content: systemPrompt },
    ...visibleMessages.map(msg => ({ role: msg.role as string, content: msg.content })),
    { role: 'user' as const, content: newContent },
  ]

  try {
    const res = await invoke<string>('llm_chat', { apiHost, apiKey, model, messages: llmMessages })
    const annotatedRes = lang === 'ja'
      ? await invoke<string>('add_furigana', { text: res, language: lang })
      : res

    const assistantId = await invoke<number>('db_add_message_in_session', {
      sessionId, role: 'assistant', content: annotatedRes
    }).catch(() => -1)

    if (mounted.current) {
      const assistant: Msg = { id: null, role: 'assistant', content: annotatedRes }
      setMessages(prev => [...prev, assistant])
      if (assistantId >= 0) {
        setMessages(prev => prev.map((msg, idx) =>
          idx === prev.length - 1 ? { ...msg, id: assistantId } : msg
        ))
      }
    }

    // Re-evaluate feedback for the edited message
    if (editedId >= 0 && apiHost && apiKey) {
      const fbId = editedId
      setFeedbackMap(prev => new Map(prev).set(fbId, { messageId: fbId, score: 0, feedback: '', loading: true }))
      const contextMsgs = visibleMessages.map(msg => ({ role: msg.role as string, content: msg.content }))
      invoke<UserFeedback>('db_evaluate_user_message', {
        apiHost, apiKey, model,
        messageId: fbId,
        messageContent: newContent,
        language: lang || '',
        level: lang === 'ja' ? jlptLevel : '',
        contextMessages: contextMsgs,
        targetWords,
      }).then(fb => {
        const userUsedAnyTarget = checkTargetWordUsage(newContent, targetWords)
        const finalScore = (!userUsedAnyTarget && targetWords.length > 0) ? 0 : fb.score
        setFeedbackMap(prev => {
          const next = new Map(prev)
          next.set(fbId, { ...fb, messageId: fbId, score: finalScore, loading: false })
          return next
        })
      }).catch(() => {
        setFeedbackMap(prev => {
          const next = new Map(prev)
          const existing = next.get(fbId)
          if (existing?.loading) next.delete(fbId)
          return next
        })
      })
    }
  } catch (e) {
    // Error handling same as send()
    const errMsg = e instanceof Error ? e.message : String(e)
    const errorContent = `[Error] ${errMsg}`
    const errorId = await invoke<number>('db_add_message_in_session', {
      sessionId, role: 'assistant', content: errorContent
    }).catch(() => -1)
    if (mounted.current) {
      const errorMsg: Msg = { id: null, role: 'assistant', content: errorContent }
      setMessages(prev => [...prev, errorMsg])
      if (errorId >= 0) {
        setMessages(prev => prev.map((msg, idx) =>
          idx === prev.length - 1 ? { ...msg, id: errorId } : msg
        ))
      }
    }
  } finally {
    if (sessionId) clearLLMPending(sessionId)
    if (mounted.current) setLoading(false)
  }
}
```

---

### 3. Superseded (Hidden) Messages — Expandable/Collapsible

**File:** `src/pages/Chat.tsx`

#### 3a. Add state for superseded messages

```ts
const [supersededMessages, setSupersededMessages] = useState<Set<number>>(new Set())
const [expandedSuperseded, setExpandedSuperseded] = useState<Set<number>>(new Set())
```

#### 3b. Render superseded messages

Superseded messages are rendered as collapsed, dimmed rows. Clicking expands them:

```tsx
// In the messages.map() section
{supersededMessages.has(m.id!) ? (
  <div key={i} className={`flex mb-1 ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}>
    <button
      onClick={() => {
        setExpandedSuperseded(prev => {
          const next = new Set(prev)
          if (next.has(m.id!)) next.delete(m.id!)
          else next.add(m.id!)
          return next
        })
      }}
      className="text-[10px] text-gray-400 hover:text-gray-500 transition-colors px-2 py-0.5"
    >
      {expandedSuperseded.has(m.id!) ? '▾ Hide previous' : '▸ Show previous'}
    </button>
    {expandedSuperseded.has(m.id!) && (
      <div className={`max-w-[80%] px-3 py-2 text-[13px] leading-relaxed opacity-50 ${
        m.role === 'user'
          ? 'bg-blue-100 text-blue-800 rounded-2xl rounded-br-md'
          : 'bg-gray-100 text-gray-600 rounded-2xl rounded-bl-md'
      }`}>
        {m.role === 'assistant' ? renderAssistantContent(m.content, m.id) : m.content}
      </div>
    )}
  </div>
) : (
  // ... normal message rendering (existing code)
)}
```

---

### 4. Add `db_update_message_content` Backend Command

**File:** `src-tauri/src/lib.rs`

New Tauri command:

```rust
#[tauri::command]
fn db_update_message_content(app: tauri::AppHandle, message_id: i64, content: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    conn.execute(
        "UPDATE messages SET content=?1 WHERE id=?2",
        params![content, message_id],
    ).map_err(|e| e.to_string())?;
    // Clear cached feedback so re-evaluation works cleanly
    conn.execute(
        "DELETE FROM user_message_feedback WHERE message_id=?1",
        params![message_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
```

Register in `invoke_handler`:
```rust
db_update_message_content,
```

---

### 5. No Changes Needed

**File:** `src/components/UserMessageFeedback.tsx`

No type changes needed — the `feedback` field is already a free-form markdown string. The new target word table and suggestion will be rendered by the existing `ReactMarkdown` with `remarkGfm` (already supports tables).

---

## Files Changed Summary

| File | Change |
|---|---|
| `src-tauri/src/lib.rs` | Add `target_words` param to `db_evaluate_user_message`; update evaluation prompt; add `db_update_message_content` command; register new command |
| `src/pages/Chat.tsx` | Extract `buildTargetWordsList()`; pass `targetWords` to feedback invoke; add score override (0 if no target words); add edit state/buttons/handler; add superseded message tracking + expandable UI; add `EditOutlined` import |
| `src/components/UserMessageFeedback.tsx` | No changes needed |

---

## Implementation Order

1. **Backend:** Add `db_update_message_content` command + register it
2. **Backend:** Add `target_words` param to `db_evaluate_user_message` + update prompt
3. **Frontend:** Extract `buildTargetWordsList()` helper from `buildSSRSystemPrompt()`
4. **Frontend:** Pass `targetWords` to feedback invoke call + add score override logic
5. **Frontend:** Add superseded message state + expandable/collapsible UI
6. **Frontend:** Add edit button, edit mode UI (empty input + "Edit" label), and `handleEditSave` handler
