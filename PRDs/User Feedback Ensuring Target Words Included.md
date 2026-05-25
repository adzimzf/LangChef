# Chat user input feedback and edit
## Background
When user answering the chat from Bot, user can answer anything. there's no enforcement for user to use the target learning words.
Impact: User doesn't use the target learning words to answer the question. Meanwhile, it's the key feature of this application to ensure the target words are stored i the user longterm memory.

## Solution
### Revamp the user chat feedback mechanism
Previously this app only use ask the LLM to provide the feedback for the chat based on grammar and vocabulary.
- in the next improvement ask the LLM also to provide whether user used one of the target words to answer the question.
- If user doesn't use the target words, provide a suggestion what user should respond included the target words.
- The feedback format must include a table to show words by words usage.
- The feedback rating must consider the target words usage too.
- If user doesn't use any of the target words, the rating must be 0. Otherwise, the rating follows the LLM judgement.

### Add button for user to edit the chat
In order for user to use the target words,
- add a button for user to edit the chat.
- when user click the edit button, it will reuse the input chat box (no pre-fill). User must retype the entire message — this reinforces learning and helps master the words.
- add a label "Edit" to indicate it's an edit mode.
- after user click send, it will re-trigger the LLM chat (new bot response) and re-evaluate the feedback.
- the previous user message must be hidden, but user can still see it (by click the arrow righ or arrow left).
- it's like a branch, below is the ilustration:
  (A)bot message -> (B) user message -> (C)bot message -> ...
                 -> (B1) user messsage [edit 1] -> (C) bot message -> (D)user message
                                                                   -> (D1) user message [edit 1]
