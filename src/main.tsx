import React from 'react'
import { createRoot } from 'react-dom/client'
import { ConfigProvider } from 'antd'
import enUS from 'antd/locale/en_US'
import idID from 'antd/locale/id_ID'
import App from './App'
import { lang, syncExplainLang } from './lib/i18n'
import './index.css'

// Rust reads the explanation language from the database (prompts, card meanings).
syncExplainLang()
document.documentElement.lang = lang

const root = createRoot(document.getElementById('root')!)
root.render(
  <ConfigProvider locale={lang === 'id' ? idID : enUS}>
    <App />
  </ConfigProvider>
)
