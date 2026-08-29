import { useEffect, useRef, useState } from 'react'
import { Toast } from './lib/api'
import { navigate, useRoute } from './lib/router'
import { ConfirmDialog, ConfirmOptions, LegalModal, Toasts } from './components/ui'
import OverviewPage from './pages/OverviewPage'
import LibraryPage from './pages/LibraryPage'
import ConsolePage from './pages/ConsolePage'
import MemoryDetailModal from './pages/MemoryDetailModal'
import GapDetailModal from './pages/GapDetailModal'

const TOKEN_KEY = 'agent-first-developer-token'

export default function App() {
  const route = useRoute()
  const [developerToken, setDeveloperToken] = useState(() => localStorage.getItem(TOKEN_KEY) ?? '')
  const [toasts, setToasts] = useState<Toast[]>([])
  const [confirmOptions, setConfirmOptions] = useState<ConfirmOptions | null>(null)
  const [legal, setLegal] = useState<'terms' | 'privacy' | 'contact' | null>(null)
  const toastId = useRef(0)
  const confirmResolver = useRef<((ok: boolean) => void) | null>(null)
  const pushedMemory = useRef(false)

  useEffect(() => { window.scrollTo(0, 0) }, [route.page])

  const addToast = (text: string, kind: 'info' | 'error' = 'info') => {
    const id = ++toastId.current
    setToasts(prev => [...prev, { id, text, kind }])
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 4000)
  }

  const copyText = async (value: string, label: string) => {
    try { await navigator.clipboard.writeText(value); addToast(`${label}已复制，请保存到安全位置。`) }
    catch { addToast('复制失败，请手动复制这段内容。', 'error') }
  }

  const confirm = (options: ConfirmOptions) => new Promise<boolean>(resolve => {
    confirmResolver.current = resolve
    setConfirmOptions(options)
  })
  const finishConfirm = (ok: boolean) => {
    confirmResolver.current?.(ok)
    confirmResolver.current = null
    setConfirmOptions(null)
  }

  const handleAuth = (token: string) => {
    localStorage.setItem(TOKEN_KEY, token)
    setDeveloperToken(token)
  }
  const handleLogout = () => {
    localStorage.removeItem(TOKEN_KEY)
    setDeveloperToken('')
    addToast('已退出登录。')
  }

  const openMemory = (id: string) => {
    pushedMemory.current = true
    navigate(`${route.page}/memory/${id}`)
  }
  const closeMemory = () => {
    if (pushedMemory.current) {
      pushedMemory.current = false
      window.history.back()
    } else {
      navigate(route.page)
    }
  }
  const pushedGap = useRef(false)
  const openGap = (id: string) => {
    pushedGap.current = true
    navigate(`${route.page}/gap/${id}`)
  }
  const closeGap = () => {
    if (pushedGap.current) {
      pushedGap.current = false
      window.history.back()
    } else {
      navigate(route.page)
    }
  }

  return <main className="shell">
    <Toasts toasts={toasts} />

    <header className="nav">
      <button type="button" className="brand" onClick={() => navigate('overview')}>Agent-first<i>.</i></button>
      <nav>
        <button type="button" className={route.page === 'overview' ? 'on' : ''} onClick={() => navigate('overview')}>概览</button>
        <button type="button" className={route.page === 'library' ? 'on' : ''} onClick={() => navigate('library')}>经验库</button>
        <button type="button" className={`console-btn ${route.page === 'console' ? 'on' : ''}`} onClick={() => navigate('console')}>控制台</button>
      </nav>
    </header>

    {route.page === 'overview' && <OverviewPage openMemory={openMemory} />}
    {route.page === 'library' && <LibraryPage token={developerToken} onToast={addToast} openMemory={openMemory} openGap={openGap} />}
    {route.page === 'console' && <ConsolePage token={developerToken} onAuth={handleAuth} onLogout={handleLogout} onToast={addToast} confirm={confirm} openMemory={openMemory} />}

    {route.memoryId && <MemoryDetailModal id={route.memoryId} token={developerToken} onClose={closeMemory} openGap={openGap} />}
    {route.gapId && <GapDetailModal id={route.gapId} token={developerToken} onClose={closeGap} openMemory={openMemory} />}

    {confirmOptions && <ConfirmDialog options={confirmOptions} onDone={finishConfirm} />}
    {legal && <LegalModal kind={legal} onClose={() => setLegal(null)} onCopy={(value, label) => void copyText(value, label)} />}

    <footer className="site-footer">
      <span>© 2026 Agent-first</span>
      <button type="button" onClick={() => setLegal('terms')}>服务条款</button>
      <button type="button" onClick={() => setLegal('privacy')}>隐私政策</button>
      <button type="button" onClick={() => setLegal('contact')}>联系方式</button>
      <span>experiencenet.dev</span>
    </footer>
  </main>
}
