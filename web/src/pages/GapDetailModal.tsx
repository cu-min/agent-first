import { useEffect, useState } from 'react'
import { Modal, MemoryCard } from '../components/ui'
import { api, authHeaders, condText, GapDetail, langText, relTime, visibilityText } from '../lib/api'

export default function GapDetailModal({ id, token, onClose, openMemory }: { id: string; token: string; onClose: () => void; openMemory: (id: string) => void }) {
  return <Modal onClose={onClose} label="缺口详情">
    <GapDetailBody id={id} token={token} onClose={onClose} openMemory={openMemory} />
  </Modal>
}

function GapDetailBody({ id, token, onClose, openMemory }: { id: string; token: string; onClose: () => void; openMemory: (id: string) => void }) {
  const [detail, setDetail] = useState<GapDetail | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setDetail(null); setError(null)
    api<GapDetail>(`/v1/gaps/${id}`, authHeaders(token))
      .then(data => { if (!cancelled) setDetail(data) })
      .catch(err => { if (!cancelled) setError(err instanceof Error ? err.message : '读取失败') })
    return () => { cancelled = true }
  }, [id, token])

  if (error) return <div className="empty">{error}</div>
  if (!detail) return <div className="detail-skeleton" aria-hidden="true">
    <div className="sk-line" style={{ width: '42%' }} />
    <div className="sk-line" style={{ width: '84%' }} />
    <div className="sk-line" style={{ width: '60%' }} />
    <div className="sk-line" style={{ width: '76%' }} />
  </div>

  const { gap, memories } = detail
  const closed = memories.length > 0
  return <article className="detail-body gap-detail">
    <button type="button" className="modal-close" onClick={onClose}>关闭</button>
    <p className="eyebrow">
      <span className={`gap-st ${closed ? 'closed' : 'open'}`}>{closed ? '已闭环' : '待解'}</span>
      {' '}{visibilityText[gap.visibility] ?? gap.visibility} · {langText[gap.language] ?? gap.language}
    </p>
    <h2>{gap.question}</h2>
    <p className="meta-line">提出于 {new Date(gap.created_at).toLocaleString()}（{relTime(gap.created_at)}）· 记为不可信内容，使用前自行核对</p>
    <h3>条件</h3><pre>{condText(gap.context)}</pre>
    {gap.attempted && <><h3>已尝试</h3><p>{gap.attempted}</p></>}
    <h3>解法（{memories.length}）</h3>
    {memories.length > 0
      ? <div className="cards gap-solutions">{memories.map(item => <MemoryCard item={item} onOpen={openMemory} key={item.id} />)}</div>
      : <p className="meta-line">还没有解法。如果解决了这个问题，写入一条经验并关联这个缺口，它就会转为已闭环。</p>}
  </article>
}
