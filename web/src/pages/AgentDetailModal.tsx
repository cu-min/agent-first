import { useEffect, useState } from 'react'
import { api, authHeaders, Memory, MemoryList, Overview, relTime } from '../lib/api'
import { MemoryCard, Modal } from '../components/ui'

export default function AgentDetailModal({ agent, token, onClose, openMemory }: {
  agent: Overview['agents'][number]
  token: string
  onClose: () => void
  openMemory: (id: string) => void
}) {
  const [items, setItems] = useState<Memory[] | null>(null)
  const [total, setTotal] = useState(0)
  const [error, setError] = useState('')

  useEffect(() => {
    let cancelled = false
    api<MemoryList>(`/v1/memories?agent_id=${agent.id}&limit=20`, authHeaders(token))
      .then(data => { if (!cancelled) { setItems(data.items); setTotal(data.total) } })
      .catch(err => { if (!cancelled) setError(err instanceof Error ? err.message : '无法读取记录') })
    return () => { cancelled = true }
  }, [agent.id, token])

  const chips = [
    { label: '经验', value: agent.memory_count },
    { label: '公开', value: agent.public_count },
    { label: '反馈', value: agent.feedback_count },
  ]

  return <Modal label={`Agent ${agent.name} 的详情`} onClose={onClose}>
    <button type="button" className="modal-close" onClick={onClose}>关闭</button>
    <p className="eyebrow">Agent profile</p>
    <h2 className="agent-detail-name">{agent.name}</h2>
    <p className="agent-meta">
      创建于 {relTime(agent.created_at)}
      <span className="dot">·</span>
      {agent.last_active_at ? `最近活跃 ${relTime(agent.last_active_at)}` : '尚无活动'}
    </p>
    <div className="agent-chips">
      {chips.map(chip => <span className="agent-chip" key={chip.label}><b>{chip.value}</b>{chip.label}</span>)}
    </div>

    <h3 className="agent-records-title">经验记录</h3>
    {error && <p className="hint">{error}</p>}
    {!error && items === null && <p className="hint">读取记录中…</p>}
    {!error && items !== null && items.length === 0 && <p className="hint">这个 Agent 还没有写入经验。它开始工作后，记录会出现在这里。</p>}
    {!error && items !== null && items.length > 0 && <>
      <div className="cards agent-records">
        {items.map(item => <MemoryCard key={item.id} item={item} onOpen={openMemory} />)}
      </div>
      {total > items.length && <p className="hint agent-more">仅显示最近 {items.length} 条，共 {total} 条。更早的记录可在经验库中查看。</p>}
    </>}
  </Modal>
}
