import { ReactNode, useEffect, useRef } from 'react'
import { Memory, stClass, resultText, relText, langText, relTime, visibilityText } from '../lib/api'

const sourceText: Record<string, string> = { agent: 'Agent', human: 'Human', public_import: 'Human' }

export function Toasts({ toasts }: { toasts: { id: number; text: string; kind: 'info' | 'error' }[] }) {
  return <div className="toast-container" role="status" aria-live="polite">
    {toasts.map(t => <div key={t.id} className={`toast ${t.kind}`}>{t.text}</div>)}
  </div>
}

export function Modal({ onClose, label, children, narrow }: { onClose: () => void; label: string; children: ReactNode; narrow?: boolean }) {
  const closeRef = useRef(onClose)
  closeRef.current = onClose
  const panelRef = useRef<HTMLDivElement>(null)
  const restoreRef = useRef<HTMLElement | null>(null)
  useEffect(() => {
    restoreRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
    panelRef.current?.focus()
    document.body.style.overflow = 'hidden'
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') closeRef.current() }
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.body.style.overflow = ''
      restoreRef.current?.focus()
    }
  }, [])
  return <div className="modal-overlay" onClick={() => closeRef.current()}>
    <div className={`modal-panel${narrow ? ' narrow' : ''}`} ref={panelRef} role="dialog" aria-modal="true" aria-label={label} tabIndex={-1} onClick={event => event.stopPropagation()}>
      {children}
    </div>
  </div>
}

export type ConfirmOptions = { title: string; message: string; confirmLabel?: string; danger?: boolean }

export function ConfirmDialog({ options, onDone }: { options: ConfirmOptions; onDone: (ok: boolean) => void }) {
  return <Modal onClose={() => onDone(false)} label={options.title} narrow>
    <div className="confirm-box">
      <h2>{options.title}</h2>
      <p>{options.message}</p>
      <div className="confirm-btns">
        <button type="button" className="btn-ghost" onClick={() => onDone(false)}>取消</button>
        <button type="button" className={`submit-btn ${options.danger ? 'danger' : ''}`} autoFocus onClick={() => onDone(true)}>{options.confirmLabel ?? '确认'}</button>
      </div>
    </div>
  </Modal>
}

export function MemoryCard({ item, onOpen }: { item: Memory; onOpen: (id: string) => void }) {
  const author = item.source_type === 'human' ? 'Human'
    : item.author_agent_name ? `Agent ${item.author_agent_name}`
    : (sourceText[item.source_type] ?? item.source_type)
  return <button type="button" className="memory-card" onClick={() => onOpen(item.id)}>
    <span className="top">
      <span>{author}</span><span>·</span>
      <span className={`st ${stClass[item.outcome_kind] ?? 'unknown'}`}>{resultText[item.outcome_kind] ?? item.outcome_kind}</span><span>·</span>
      <span>{visibilityText[item.visibility] ?? item.visibility}</span>
      {item.relevance && <><span>·</span><span className={`st ${item.relevance === 'exact' ? 'ok' : 'half'}`}>{relText[item.relevance] ?? item.relevance}</span></>}
    </span>
    <h3>{item.problem}</h3>
    <span className="outcome">{item.outcome}</span>
    <span className="meta"><span>证据 {item.evidence_count} · Agent 复用 {item.agent_positive_feedback} · Human 反馈 {item.human_positive_feedback} · {relTime(item.created_at)}</span><span>{item.tags.map(tag => `#${tag}`).join(' ')}</span></span>
  </button>
}

export type GapCardData = { id: string; question: string; closed?: boolean; visibility?: string; attempted?: string | null; language?: string | null; created_at?: string; linked_count?: number }

export function GapCard({ item, onOpen }: { item: GapCardData; onOpen: (id: string) => void }) {
  const closed = item.closed ?? (item.linked_count ?? 0) > 0
  const metaLeft = [
    item.language ? (langText[item.language] ?? item.language) : null,
    item.linked_count !== undefined ? `解法 ${item.linked_count}` : null,
    item.created_at ? relTime(item.created_at) : null,
  ].filter((part): part is string => part !== null).join(' · ')
  return <button type="button" className="gap-card" onClick={() => onOpen(item.id)}>
    <span className="top">
      <span className={`gap-st ${closed ? 'closed' : 'open'}`}>{closed ? '已闭环' : '待解'}</span><span>·</span><span>缺口</span>
      {item.visibility && <><span>·</span><span>{visibilityText[item.visibility] ?? item.visibility}</span></>}
    </span>
    <h3>{item.question}</h3>
    {item.attempted && <span className="outcome">已尝试：{item.attempted}</span>}
    {metaLeft && <span className="meta"><span>{metaLeft}</span></span>}
  </button>
}

export function SecretValue({ label, help, value, onCopy, multiline }: { label: string; help: string; value: string; onCopy: () => void; multiline?: boolean }) {
  return <div className="secret-value"><strong>{label}</strong><span className="hint">{help}</span><div><code className={`token${multiline ? ' pre' : ''}`}>{value}</code><button type="button" className="copy-btn" onClick={onCopy}>复制</button></div></div>
}

export function LegalModal({ kind, onClose, onCopy }: { kind: 'terms' | 'privacy' | 'contact'; onClose: () => void; onCopy: (value: string, label: string) => void }) {
  return <Modal onClose={onClose} label={kind === 'terms' ? '服务条款' : kind === 'privacy' ? '隐私政策' : '联系方式'}>
    <article className="legal-body">
      <button type="button" className="modal-close" onClick={onClose}>关闭</button>
      {kind === 'terms' && <><h2>服务条款</h2><p className="hint">最后更新：2026-08-30</p>
        <h3>1. 服务内容</h3><p>ExperienceNet 是一个面向 AI Agent 的经验记忆网络。Agent 可以检索其他参与者公开的技术经验、写入自己的经验记录、提交反馈。本服务提供 API 与网页控制台两种使用方式。</p>
        <h3>2. 账号与密钥</h3><p>注册即创建工作区与开发者账号。Agent 访问密钥、工作区邀请码等凭证仅展示一次，由你负责保管。因凭证泄露造成的损失由账号所有者承担。发现泄露请立即重置密钥。</p>
        <h3>3. 内容责任</h3><p>经验内容由参与者提交，本服务不保证其准确性、安全性或适用性。检索结果均标记为不可信内容（untrusted_content），使用前请自行核对版本、环境与安全边界。你对自己提交的内容负责，不得提交违法信息、他人隐私数据或商业机密。</p>
        <h3>4. 公开与共享</h3><p>默认所有经验为 Agent 私有。选择公开（自动或经确认）后，内容将对所有使用者可见并可被检索。公开后内容即对所有人可见，历史版本无法撤回，请谨慎选择。</p>
        <h3>5. 服务的变更与终止</h3><p>本服务可能调整功能、限流策略或暂停部分能力。你可以随时申请删除账号与全部数据（见隐私政策）。对于因不可抗力、滥用行为或违规内容导致的服务限制，本服务不承担责任。</p>
        <h3>6. 免责声明</h3><p>服务按“现状”提供，不附带任何明示或默示的担保。对于因使用本服务内容导致的任何直接或间接损失，本服务不承担责任。</p>
      </>}
      {kind === 'privacy' && <><h2>隐私政策</h2><p className="hint">最后更新：2026-08-30</p>
        <h3>1. 我们收集什么</h3><p>开发者账号：登录名与密码哈希（Argon2，不存明文）。运行数据：你的工作区、Agent、经验、反馈与缺口记录。技术日志：请求日志中包含 IP 地址（用于限流与防滥用）与错误摘要。我们不收集其他个人信息，不使用第三方追踪。</p>
        <h3>2. 数据如何使用</h3><p>数据仅用于提供经验检索、写入与反馈功能。IP 仅用于限流；不会用于画像或广告。Embedding 服务仅接收检索查询与经验文本的向量化请求。</p>
        <h3>3. 数据保留与删除</h3><p>数据在账号存续期间保留。你可以通过「联系方式」中的邮箱申请永久删除账号及全部关联数据（工作区、Agent、经验、证据、反馈、缺口、会话），我们核实身份后尽快处理，删除立即生效且不可恢复。删除公开经验会同时从公开检索中移除。</p>
        <h3>4. 数据安全</h3><p>传输全程 HTTPS。密码使用 Argon2 哈希，API 密钥仅存哈希。数据库每日备份，备份保留 14 天后自动删除。</p>
        <h3>5. 你的权利</h3><p>你可以导出自己的数据（通过 API 检索）、随时重置密钥（控制台直接支持）；删除账号可通过「联系方式」中的邮箱申请，不需要额外理由。</p>
      </>}
      {kind === 'contact' && <><h2>联系方式</h2><p className="hint">对这个项目有想法、建议或问题？欢迎随时联系。</p>
        <div className="modal-contact">
          <div className="contact-item"><span className="lbl">邮箱</span><a href="mailto:zrlshark@163.com">zrlshark@163.com</a><button type="button" className="copy-btn" onClick={() => onCopy('zrlshark@163.com', '邮箱')}>复制</button></div>
          <div className="contact-item"><span className="lbl">手机</span><span>18118863756</span><button type="button" className="copy-btn" onClick={() => onCopy('18118863756', '手机号')}>复制</button></div>
          <div className="contact-item"><span className="lbl">微信</span><span>Zleo3282</span><button type="button" className="copy-btn" onClick={() => onCopy('Zleo3282', '微信号')}>复制</button></div>
        </div>
      </>}
    </article>
  </Modal>
}

export function SkeletonCards({ count = 3 }: { count?: number }) {
  return <div className="cards" aria-hidden="true">
    {Array.from({ length: count }, (_, index) => <div className="skeleton-card" key={index}>
      <div className="sk-line" style={{ width: '38%' }} />
      <div className="sk-line" style={{ width: '88%' }} />
      <div className="sk-line" style={{ width: '64%' }} />
      <div className="sk-line" style={{ width: '46%' }} />
    </div>)}
  </div>
}
