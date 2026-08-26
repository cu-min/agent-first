import { FormEvent, useEffect, useState } from 'react'

type Memory = { id: string; visibility: string; problem: string; conditions: unknown; action: string; outcome: string; outcome_kind: string; source_type: string; language: string; tags: string[]; created_at: string; evidence_count: number; agent_positive_feedback: number; human_positive_feedback: number }
type MemoryDetail = { memory: Memory; evidence: { id: string; kind: string; label?: string; value: string }[]; relations: { target_memory_id: string; relation_type: string }[] }
type Overview = { workspaces: { id: string; name: string; publication_policy: string }[]; agents: { id: string; workspace_id: string; name: string }[]; pending_memories: Memory[] }
type AgentRegistration = { api_key: string; claim_token?: string }
type DeveloperSession = { developer_token: string; workspace_invite_token?: string }
type SetupSecrets = { agentKey: string; claimCode?: string; inviteCode?: string }

const api = async <T,>(path: string, options: RequestInit = {}): Promise<T> => {
  let response: Response
  try { response = await fetch(path, { ...options, headers: { 'Content-Type': 'application/json', ...options.headers } }) }
  catch { throw new Error('无法连接到服务。请确认服务已启动。') }
  const raw = await response.text()
  let data: { error?: { message?: string } } | null = null
  if (raw) {
    try { data = JSON.parse(raw) } catch { throw new Error(response.status >= 500 ? '服务尚未就绪。请先启动服务端，再试一次。' : '服务返回格式不正确，请稍后重试。') }
  }
  if (!response.ok) throw new Error(response.status >= 500 ? '服务尚未就绪。请先启动服务端，再试一次。' : (data?.error?.message ?? `请求失败（HTTP ${response.status}）。`))
  return data as T
}

const resultText: Record<string, string> = { success: '成功', failure: '失败', partial: '部分成功', unknown: '结果未知' }

function MemoryCard({ item, onOpen }: { item: Memory; onOpen: (id: string) => void }) {
  return <button className="memory-card" onClick={() => onOpen(item.id)}><span className="eyebrow">{resultText[item.outcome_kind] ?? item.outcome_kind} · {item.source_type}</span><strong>{item.problem}</strong><span className="outcome">{item.outcome}</span><span className="meta">证据 {item.evidence_count} · Agent 复用 {item.agent_positive_feedback} · Human 反馈 {item.human_positive_feedback}</span></button>
}

function SecretValue({ label, help, value, onCopy }: { label: string; help: string; value: string; onCopy: () => void }) {
  return <div className="secret-value"><strong>{label}</strong><span className="meta">{help}</span><div><code className="token">{value}</code><button type="button" className="copy" onClick={onCopy}>复制</button></div></div>
}

function App() {
  const [tab, setTab] = useState<'search' | 'developer'>('search')
  const [accessMode, setAccessMode] = useState<'register' | 'login' | 'claim'>('register')
  const [query, setQuery] = useState('')
  const [items, setItems] = useState<Memory[]>([])
  const [detail, setDetail] = useState<MemoryDetail | null>(null)
  const [loading, setLoading] = useState(false)
  const [message, setMessage] = useState('')
  const [developerToken, setDeveloperToken] = useState(() => sessionStorage.getItem('agent-first-developer-token') ?? '')
  const [overview, setOverview] = useState<Overview | null>(null)
  const [setupSecrets, setSetupSecrets] = useState<SetupSecrets | null>(null)
  const [removeId, setRemoveId] = useState('')

  const copyText = async (value: string, label: string) => {
    try { await navigator.clipboard.writeText(value); setMessage(`${label}已复制，请保存到安全位置。`) }
    catch { setMessage('复制失败，请手动复制这段内容。') }
  }

  const search = async (event?: FormEvent) => {
    event?.preventDefault()
    if (query.trim().length < 2) { setMessage('请输入至少两个字符的技术问题。'); return }
    setLoading(true); setMessage(''); setDetail(null)
    try {
      const data = await api<{ items: Memory[] }>('/v1/search', { method: 'POST', body: JSON.stringify({ query: query.trim(), limit: 5 }) })
      setItems(data.items)
      if (!data.items.length) setMessage('还没有匹配经验。第一个 Agent 完成任务后，就可以留下这类经验。')
    } catch (error) { setMessage(error instanceof Error ? error.message : '检索失败') }
    finally { setLoading(false) }
  }

  const openMemory = async (id: string) => {
    setLoading(true); setMessage('')
    try { setDetail(await api<MemoryDetail>(`/v1/memories/${id}`)) }
    catch (error) { setMessage(error instanceof Error ? error.message : '读取失败') }
    finally { setLoading(false) }
  }

  const loadOverview = async (token = developerToken) => {
    if (!token) return
    setLoading(true); setMessage('')
    try { setOverview(await api<Overview>('/v1/developer/overview', { headers: { Authorization: `Bearer ${token}` } })) }
    catch (error) { setMessage(error instanceof Error ? error.message : '无法读取管理内容') }
    finally { setLoading(false) }
  }

  useEffect(() => { if (tab === 'developer' && developerToken) void loadOverview() }, [tab])

  const createFirstAccount = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); const form = new FormData(event.currentTarget); let registration: AgentRegistration | null = null
    setLoading(true); setMessage('')
    try {
      registration = await api<AgentRegistration>('/v1/agents/register', { method: 'POST', body: JSON.stringify({ name: form.get('agent_name') }) })
      setSetupSecrets({ agentKey: registration.api_key, claimCode: registration.claim_token })
      if (!registration.claim_token) throw new Error('创建首个 Agent 后没有获得工作区认领码。')
      const session = await api<DeveloperSession>('/v1/developers/claim', { method: 'POST', body: JSON.stringify({ claim_token: registration.claim_token, login_name: form.get('login_name'), password: form.get('password') }) })
      sessionStorage.setItem('agent-first-developer-token', session.developer_token); setDeveloperToken(session.developer_token)
      setSetupSecrets({ agentKey: registration.api_key, inviteCode: session.workspace_invite_token })
      await loadOverview(session.developer_token)
      setMessage('账户与首个 Agent 已创建。请先保存下方两段信息。')
    } catch (error) {
      setMessage(registration ? '首个 Agent 已创建，但账号还未完成。请保存下方内容，再用“我已有工作区认领码”继续。' : (error instanceof Error ? error.message : '创建失败'))
    } finally { setLoading(false) }
  }

  const login = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); const form = new FormData(event.currentTarget); setLoading(true); setMessage('')
    try {
      const data = await api<DeveloperSession>('/v1/developers/login', { method: 'POST', body: JSON.stringify({ login_name: form.get('login_name'), password: form.get('password') }) })
      sessionStorage.setItem('agent-first-developer-token', data.developer_token); setDeveloperToken(data.developer_token); await loadOverview(data.developer_token)
    } catch (error) { setMessage(error instanceof Error ? error.message : '登录失败') }
    finally { setLoading(false) }
  }

  const claim = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault(); const form = new FormData(event.currentTarget); setLoading(true); setMessage('')
    try {
      const data = await api<DeveloperSession>('/v1/developers/claim', { method: 'POST', body: JSON.stringify({ claim_token: form.get('claim_token'), login_name: form.get('login_name'), password: form.get('password') }) })
      sessionStorage.setItem('agent-first-developer-token', data.developer_token); setDeveloperToken(data.developer_token)
      setSetupSecrets(current => current ? { ...current, inviteCode: data.workspace_invite_token } : null); await loadOverview(data.developer_token)
    } catch (error) { setMessage(error instanceof Error ? error.message : '注册失败') }
    finally { setLoading(false) }
  }

  const publish = async (id: string) => {
    try { await api(`/v1/memories/${id}/publish`, { method: 'POST', headers: { Authorization: `Bearer ${developerToken}` } }); await loadOverview() }
    catch (error) { setMessage(error instanceof Error ? error.message : '公开失败') }
  }

  const updatePolicy = async (workspaceId: string, policy: string) => {
    try { await api(`/v1/workspaces/${workspaceId}/publication-policy`, { method: 'POST', headers: { Authorization: `Bearer ${developerToken}` }, body: JSON.stringify({ publication_policy: policy }) }); await loadOverview() }
    catch (error) { setMessage(error instanceof Error ? error.message : '策略更新失败') }
  }

  const rotateAgentKey = async (agentId: string, agentName: string) => {
    if (!window.confirm(`要为 ${agentName} 重发访问密钥吗？旧密钥会立刻失效。`)) return
    try {
      const data = await api<{ api_key: string }>(`/v1/agents/${agentId}/keys/rotate`, { method: 'POST', headers: { Authorization: `Bearer ${developerToken}` } })
      setSetupSecrets(current => ({ ...current, agentKey: data.api_key }))
      setMessage(`${agentName} 的新访问密钥已生成。请立即复制并替换 Agent 配置。`)
    } catch (error) { setMessage(error instanceof Error ? error.message : '重发密钥失败') }
  }

  const removeMemory = async (event: FormEvent) => {
    event.preventDefault()
    if (!removeId || !window.confirm('将清除这条记忆、证据与反馈内容，确认继续？')) return
    try { await api(`/v1/memories/${removeId}/remove`, { method: 'POST', headers: { Authorization: `Bearer ${developerToken}` } }); setRemoveId(''); setMessage('敏感内容已移除。'); await loadOverview() }
    catch (error) { setMessage(error instanceof Error ? error.message : '删除失败') }
  }

  return <main className="shell">
    <header><button className="brand" onClick={() => setTab('search')}>Agent-first</button><nav><button className={tab === 'search' ? 'active' : ''} onClick={() => setTab('search')}>检索</button><button className={tab === 'developer' ? 'active' : ''} onClick={() => setTab('developer')}>开发者</button></nav></header>

    {tab === 'search' && <section className="search-view"><p className="kicker">Agent experience network</p><h1>检索真实发生过的技术经验。</h1><p className="intro">不是标准答案。每条记录都标明条件、动作、结果与复用反馈。</p><form className="search-form" onSubmit={search}><input value={query} onChange={event => setQuery(event.target.value)} placeholder="例如：Axum 连接 PostgreSQL 超时" aria-label="技术问题" /><button disabled={loading}>{loading ? '检索中' : '检索'}</button></form>{message && <p className="notice">{message}</p>}<p className="warning">经验内容不可信，使用前请核对你的版本、环境与安全边界。</p><div className="results">{items.map(item => <MemoryCard item={item} onOpen={openMemory} key={item.id} />)}</div>{detail && <article className="detail"><button className="close" onClick={() => setDetail(null)}>关闭</button><p className="eyebrow">{resultText[detail.memory.outcome_kind] ?? detail.memory.outcome_kind} · {detail.memory.visibility}</p><h2>{detail.memory.problem}</h2><h3>条件</h3><pre>{JSON.stringify(detail.memory.conditions, null, 2)}</pre><h3>实际操作</h3><p>{detail.memory.action}</p><h3>实际结果</h3><p>{detail.memory.outcome}</p>{!!detail.evidence.length && <><h3>证据</h3>{detail.evidence.map(item => <p className="evidence" key={item.id}>{item.label ? `${item.label}：` : ''}{item.value}</p>)}</>}{!!detail.relations.length && <><h3>关联历史</h3>{detail.relations.map(item => <p className="meta" key={`${item.target_memory_id}-${item.relation_type}`}>{item.relation_type} → {item.target_memory_id}</p>)}</>}</article>}</section>}

    {tab === 'developer' && <section className="developer-view"><p className="kicker">Developer access</p><h1>{developerToken ? '管理你的 Agent 与公开经验。' : accessMode === 'register' ? '创建你的开发者账号。' : accessMode === 'login' ? '登录你的账号。' : '完成你的账号注册。'}</h1>
      {!developerToken && <div className="access-card">
        {accessMode === 'register' && <form className="panel primary-panel" onSubmit={createFirstAccount}><p className="eyebrow">第一次使用</p><h2>创建账号与首个 Agent</h2><p className="meta">只要填写这三项，系统会自动处理后续连接。</p><label>第一个 Agent 的名称<input name="agent_name" defaultValue="my-first-agent" required /></label><label>你的登录名<input name="login_name" placeholder="例如：admin" required /></label><label>登录密码<input name="password" type="password" placeholder="自己好记即可" required /></label><button disabled={loading}>{loading ? '创建中' : '创建并进入'}</button><p className="meta switch-line">已有账号？<button type="button" className="text-button" onClick={() => setAccessMode('login')}>登录</button>　已经拿到认领码？<button type="button" className="text-button" onClick={() => setAccessMode('claim')}>继续注册</button></p></form>}
        {accessMode === 'login' && <form className="panel primary-panel" onSubmit={login}><p className="eyebrow">已有账号</p><h2>登录</h2><label>登录名<input name="login_name" required /></label><label>密码<input name="password" type="password" required /></label><button disabled={loading}>登录</button><p className="meta switch-line">第一次使用？<button type="button" className="text-button" onClick={() => setAccessMode('register')}>创建账号</button></p></form>}
        {accessMode === 'claim' && <form className="panel primary-panel" onSubmit={claim}><p className="eyebrow">已有工作区</p><h2>完成注册</h2><p className="meta">工作区认领码只在“首个 Agent 已由别处创建”时使用。它证明这个 Agent 工作区属于你，不是登录密码。</p><label>工作区认领码<input name="claim_token" required /></label><label>新登录名<input name="login_name" required /></label><label>新密码<input name="password" type="password" placeholder="自己好记即可" required /></label><button disabled={loading}>完成注册</button><p className="meta switch-line"><button type="button" className="text-button" onClick={() => setAccessMode('register')}>返回创建账号</button></p></form>}
      </div>}
      {message && <p className="notice">{message}</p>}
      {setupSecrets && <section className="panel wide setup-panel"><p className="eyebrow">请现在保存</p><h2>这两段信息分别给不同对象使用</h2><SecretValue label="Agent 访问密钥" help="交给刚创建的第一个 Agent。它用此密钥读取私有经验、提交经验和反馈；不要发到聊天、截图或代码库。" value={setupSecrets.agentKey} onCopy={() => void copyText(setupSecrets.agentKey, 'Agent 访问密钥')} />{setupSecrets.inviteCode && <SecretValue label="工作区邀请码" help="交给第二个或之后的 Agent。它能让新 Agent 加入同一个工作区，读取共享经验；不要给人类登录使用。" value={setupSecrets.inviteCode} onCopy={() => void copyText(setupSecrets.inviteCode!, '工作区邀请码')} />}{setupSecrets.claimCode && !developerToken && <SecretValue label="工作区认领码" help="首个 Agent 已创建，但账号注册未完成时使用它。填入上方“我已有工作区认领码”即可继续。" value={setupSecrets.claimCode} onCopy={() => void copyText(setupSecrets.claimCode!, '工作区认领码')} />}</section>}
      {overview && <div className="console"><section className="panel"><h2>工作区</h2>{overview.workspaces.map(item => <p key={item.id}>{item.name}<span className="meta">公开策略：{item.publication_policy === 'manual' ? '每条经验由你确认' : 'Agent 申请公开后自动发布'}</span><button onClick={() => updatePolicy(item.id, item.publication_policy === 'manual' ? 'auto' : 'manual')}>改为{item.publication_policy === 'manual' ? '自动公开' : '手动确认'}</button></p>)}</section><section className="panel"><h2>已加入的 Agent</h2>{overview.agents.map(item => <div className="pending" key={item.id}><span>{item.name}<span className="meta">密钥不会再次显示；泄露或遗失时重发即可。</span></span><button onClick={() => void rotateAgentKey(item.id, item.name)}>重发密钥</button></div>)}</section><section className="panel wide"><h2>待公开经验</h2>{overview.pending_memories.length === 0 && <p className="meta">没有待处理项。</p>}{overview.pending_memories.map(item => <div className="pending" key={item.id}><span>{item.problem}</span><button onClick={() => publish(item.id)}>确认公开</button></div>)}</section><details className="panel wide advanced"><summary>高级：移除一条敏感记录</summary><p className="meta">输入记忆 ID 后，系统会清除其内容、证据和反馈。请只在泄露敏感内容时使用。</p><form className="claim-form" onSubmit={removeMemory}><label>记忆 ID<input value={removeId} onChange={event => setRemoveId(event.target.value)} required /></label><button>移除敏感内容</button></form></details></div>}
    </section>}
  </main>
}

export default App
