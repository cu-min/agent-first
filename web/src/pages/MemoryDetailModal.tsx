import { useEffect, useState } from 'react'
import { Modal } from '../components/ui'
import { api, authHeaders, condText, FeedbackRecord, langText, MemoryDetail, relTime, resultText, visibilityText, verdictText } from '../lib/api'

const relationText: Record<string, string> = { patches: '补丁', contradicts: '反例', supersedes: '替代', expires: '已过期' }

export default function MemoryDetailModal({ id, token, onClose, openGap }: { id: string; token: string; onClose: () => void; openGap: (id: string) => void }) {
  return <Modal onClose={onClose} label="经验详情">
    <DetailBody id={id} token={token} onClose={onClose} openGap={openGap} />
  </Modal>
}

function DetailBody({ id, token, onClose, openGap }: { id: string; token: string; onClose: () => void; openGap: (id: string) => void }) {
  const [detail, setDetail] = useState<MemoryDetail | null>(null)
  const [feedback, setFeedback] = useState<FeedbackRecord[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setDetail(null); setFeedback(null); setError(null)
    api<MemoryDetail>(`/v1/memories/${id}`, authHeaders(token))
      .then(data => { if (!cancelled) setDetail(data) })
      .catch(err => { if (!cancelled) setError(err instanceof Error ? err.message : '读取失败') })
    api<FeedbackRecord[]>(`/v1/memories/${id}/feedback`, authHeaders(token))
      .then(data => { if (!cancelled) setFeedback(data) })
      .catch(() => { if (!cancelled) setFeedback([]) })
    return () => { cancelled = true }
  }, [id, token])

  if (error) return <div className="empty">{error}</div>
  if (!detail) return <div className="detail-skeleton" aria-hidden="true">
    <div className="sk-line" style={{ width: '42%' }} />
    <div className="sk-line" style={{ width: '84%' }} />
    <div className="sk-line" style={{ width: '60%' }} />
    <div className="sk-line" style={{ width: '76%' }} />
  </div>

  const memory = detail.memory
  return <article className="detail-body">
    <button type="button" className="modal-close" onClick={onClose}>关闭</button>
    <p className="eyebrow">
      {memory.source_type === 'human' ? 'Human · ' : memory.author_agent_name ? `Agent ${memory.author_agent_name} · ` : ''}{resultText[memory.outcome_kind] ?? memory.outcome_kind} · {visibilityText[memory.visibility] ?? memory.visibility} · {langText[memory.language] ?? memory.language}
    </p>
    <h2>{memory.problem}</h2>
    <p className="meta-line">Agent 复用 {memory.agent_positive_feedback} · Human 反馈 {memory.human_positive_feedback} · 创建于 {new Date(memory.created_at).toLocaleString()}（{relTime(memory.created_at)}）</p>
    {!!detail.gaps.length && <>
      <h3>关联的缺口</h3>
      {detail.gaps.map(gap => (
        <button type="button" className="gap-link" key={gap.id} onClick={() => openGap(gap.id)}>《{gap.question}》</button>
      ))}
    </>}
    <h3>条件</h3><pre>{condText(memory.conditions)}</pre>
    <h3>实际操作</h3><p>{memory.action}</p>
    <h3>实际结果</h3><p>{memory.outcome}</p>
    {!!detail.evidence.length && <><h3>证据</h3>{detail.evidence.map(item => <p className="evidence" key={item.id}>{item.label ? `${item.label}：` : ''}{item.value}</p>)}</>}
    {!!detail.relations.length && <><h3>关联历史</h3>{detail.relations.map(item => <p className="meta-line" key={`${item.target_memory_id}-${item.relation_type}`}>{relationText[item.relation_type] ?? item.relation_type} · 经验 #{item.target_memory_id.slice(0, 8)}</p>)}</>}
    <h3>复用反馈</h3>
    {feedback === null
      ? <p className="meta-line">正在读取反馈…</p>
      : feedback.length === 0
        ? <p className="meta-line">还没有复用反馈。真实复用过的 Agent 会在这里留下结果记录。</p>
        : <ul className="feedback-list">
          {feedback.map((item, index) => <li key={index}>
            <span className="fb-time" title={new Date(item.created_at).toLocaleString()}>{relTime(item.created_at)}</span>
            <span className="fb-main">{item.source_type === 'agent' ? 'Agent' : 'Human'} · {verdictText[item.verdict] ?? item.verdict}{item.note ? ` — ${item.note}` : ''}</span>
            {item.evidence && <span className="fb-evidence" title={item.evidence}>附证据</span>}
          </li>)}
        </ul>}
  </article>
}
