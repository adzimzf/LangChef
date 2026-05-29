import { BrowserRouter, Route, Routes, Navigate } from 'react-router-dom'
import Home from './pages/Home'
import Chat from './pages/Chat'
import Chats from './pages/Chats'
import Settings from './pages/Settings'
import Review from './pages/Review'
import Telemetry from './pages/Telemetry'
import UserStats from './pages/UserStats'
import WordsList from './pages/WordsList'
import Grammar from './pages/Grammar'
import Reading from './pages/Reading'
import Layout from './components/Layout'

export default function App() {
  return (
    <BrowserRouter>
      <Layout>
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/chats" element={<Chats />} />
          <Route path="/chat" element={<Chat />} />
          <Route path="/review" element={<Review />} />
          <Route path="/words" element={<WordsList />} />
          <Route path="/reading" element={<Reading />} />
          <Route path="/grammar" element={<Grammar />} />
          <Route path="/stats" element={<UserStats />} />
          <Route path="/telemetry" element={<Navigate to="/stats" replace />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </Layout>
    </BrowserRouter>
  )
}
