import { useEffect, useRef, useState } from 'react'
import { api, Overview } from '../lib/api'

// 深色底头像配色：按登录名哈希稳定取色，均为低明度暖调，保证白色机器人图标清晰
const AVATAR_COLORS = ['#bf4d24', '#2f5d50', '#8a5a24', '#7a4a52', '#3d5a6c', '#6b5d2f']

function colorFor(name: string) {
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  return AVATAR_COLORS[hash % AVATAR_COLORS.length]
}

function RobotIcon() {
  return <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <line x1="12" y1="3.6" x2="12" y2="6.2" stroke="#fff8f2" strokeWidth="1.7" strokeLinecap="round" />
    <circle cx="12" cy="3" r="1.5" fill="#fff8f2" />
    <rect x="5" y="6.5" width="14" height="11.5" rx="3.6" stroke="#fff8f2" strokeWidth="1.7" />
    <circle cx="9.6" cy="12" r="1.4" fill="#fff8f2" />
    <circle cx="14.4" cy="12" r="1.4" fill="#fff8f2" />
    <path d="M9.8 15.6h4.4" stroke="#fff8f2" strokeWidth="1.7" strokeLinecap="round" />
    <path d="M3.6 10.8v3.4M20.4 10.8v3.4" stroke="#fff8f2" strokeWidth="1.7" strokeLinecap="round" />
  </svg>
}

export default function AvatarMenu({ token, name, onLogout }: { token: string; name: string; onLogout: () => void }) {
  const [open, setOpen] = useState(false)
  const [info, setInfo] = useState<{ agents: number; workspaces: number } | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const fetchedRef = useRef(false)

  // 点击空白或按 Esc 收起
  useEffect(() => {
    if (!open) return
    const onPointer = (event: MouseEvent) => { if (!rootRef.current?.contains(event.target as Node)) setOpen(false) }
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') setOpen(false) }
    document.addEventListener('click', onPointer)
    document.addEventListener('keydown', onKey)
    return () => { document.removeEventListener('click', onPointer); document.removeEventListener('keydown', onKey) }
  }, [open])

  // 首次展开时拉一次工作区概览（失败静默降级，只显示登录名）
  useEffect(() => {
    if (!open || fetchedRef.current) return
    fetchedRef.current = true
    api<Overview>('/v1/developer/overview', { headers: { Authorization: `Bearer ${token}` } })
      .then(data => setInfo({ agents: data.agents.length, workspaces: data.workspaces.length }))
      .catch(() => {})
  }, [open, token])

  const displayName = name || '开发者'

  return <div className="avatar-wrap" ref={rootRef}>
    <button
      type="button"
      className={`avatar ${open ? 'on' : ''}`}
      style={{ background: colorFor(displayName) }}
      aria-expanded={open}
      aria-label="个人菜单"
      onClick={() => setOpen(value => !value)}
    >
      <RobotIcon />
    </button>
    {open && <div className="avatar-pop" role="menu">
      <div className="pop-head">
        <p className="pop-name">{displayName}</p>
        <p className="pop-sub">{info ? `我的工作区 · ${info.agents} 个 Agent` : '我的工作区'}</p>
      </div>
      <div className="pop-sep" />
      <button type="button" role="menuitem" className="pop-item danger" onClick={onLogout}>
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path d="M6 2H3.5A1.5 1.5 0 0 0 2 3.5v9A1.5 1.5 0 0 0 3.5 14H6M10 11.5 13.5 8 10 4.5M13.5 8H6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        退出登录
      </button>
    </div>}
  </div>
}
