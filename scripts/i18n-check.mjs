// Checks the translation dictionaries (src/locales) against each other and the code:
//  1. en.json and id.json have exactly the same keys
//  2. every t('literal.key') used in src/ exists in en.json
//  3. no hardcoded user-visible text is left in .tsx files (JSX text, and string
//     placeholder / title / aria-label / alt attributes)
// Run with `npm run i18n:check`. Exits non-zero on any problem.
import fs from 'node:fs'
import path from 'node:path'
import ts from 'typescript'

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..')
const src = path.join(root, 'src')
const en = JSON.parse(fs.readFileSync(path.join(src, 'locales/en.json'), 'utf8'))
const id = JSON.parse(fs.readFileSync(path.join(src, 'locales/id.json'), 'utf8'))

// Not rendered by any route (/telemetry redirects to /stats).
const SKIP_FILES = new Set(['pages/Telemetry.tsx'])
// Text that reads the same in every language.
const ALLOWED_TEXT = /^[\s\d\p{P}\p{S}]*$|^(LangChef|OpenRouter|Google Drive|JLPT|N[1-5]|ふりがな|ふ)$|^&#x?[0-9a-f]+;$/iu
// Technical placeholders with no spaces (URLs, key prefixes, model ids) read the same everywhere.
const TECHNICAL_VALUE = /^[\w.:/-]+$/
const TEXT_ATTRS = new Set(['placeholder', 'title', 'aria-label', 'alt'])

const problems = []

for (const key of Object.keys(en)) if (!(key in id)) problems.push(`id.json is missing "${key}"`)
for (const key of Object.keys(id)) if (!(key in en)) problems.push(`id.json has extra key "${key}"`)
for (const [key, value] of Object.entries(id)) {
  if (typeof value !== 'string' || !value.trim()) problems.push(`id.json "${key}" is empty`)
}

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name)
    return e.isDirectory() ? walk(p) : /\.(tsx?|mts)$/.test(e.name) && !e.name.endsWith('.d.ts') ? [p] : []
  })
}

// A plural key may exist only as key_one / key_other.
const hasKey = (k) => k in en || (`${k}_one` in en && `${k}_other` in en)

for (const file of walk(src)) {
  const rel = path.relative(src, file)
  const text = fs.readFileSync(file, 'utf8')
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const where = (node) => `${rel}:${sf.getLineAndCharacterOfPosition(node.getStart()).line + 1}`

  const visit = (node) => {
    // t('key') with a literal key
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 't') {
      const arg = node.arguments[0]
      if (arg && ts.isStringLiteral(arg) && !hasKey(arg.text)) problems.push(`${where(node)} unknown key "${arg.text}"`)
    }
    if (rel.endsWith('.tsx') && !SKIP_FILES.has(rel)) {
      if (ts.isJsxText(node)) {
        const value = node.getText().trim()
        if (value && /\p{L}{2,}/u.test(value) && !ALLOWED_TEXT.test(value)) {
          problems.push(`${where(node)} hardcoded text "${value.slice(0, 60)}"`)
        }
      }
      if (ts.isJsxAttribute(node) && TEXT_ATTRS.has(node.name.getText()) && node.initializer && ts.isStringLiteral(node.initializer)) {
        const value = node.initializer.text
        if (/\p{L}{2,}/u.test(value) && !ALLOWED_TEXT.test(value) && !TECHNICAL_VALUE.test(value)) {
          problems.push(`${where(node)} hardcoded ${node.name.getText()}="${value.slice(0, 60)}"`)
        }
      }
    }
    ts.forEachChild(node, visit)
  }
  visit(sf)
}

if (problems.length) {
  console.error(problems.join('\n'))
  console.error(`\ni18n check failed: ${problems.length} problem(s)`)
  process.exit(1)
}
console.log(`i18n check passed: ${Object.keys(en).length} keys in en.json and id.json`)
