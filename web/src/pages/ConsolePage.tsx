import { FormEvent, useEffect, useState } from 'react'
import { api, AgentRegistration, DeveloperSession, relTime, resultText, Overview, SetupSecrets } from '../lib/api'
import { ConfirmOptions, SecretValue } from '../components/ui'
import { navigate } from '../lib/router'
import { checkPassword } from '../lib/password'

type AccessMode = 'register' | 'login' | 'claim'

export default function ConsolePage({ token, onAuth, onLogout, onToast, confirm }: {
  token: string
  onAuth: (token: string) => void
  onLogout: () => void
  onToast: (text: string, kind?: 'info' | 'error') => void
  confirm: (options: ConfirmOptions) => Promise<boolean>
}) {
  const [accessMode, setAccessMode] = useState<AccessMode>('register')
  const [overview, setOverview] = useState<Overview | null>(null)
  const [setupSecrets, setSetupSecrets] = useState<SetupSecrets | null>(null)
  const [removeId, setRemoveId] = useState('')
  const [deletePassword, setDeletePassword] = useState('')
  const [deleteConfirmText, setDeleteConfirmText] = useState('')
  const [loading, setLoading] = useState(false)
  const [editingAgent, setEditingAgent] = useState<{ id: string; name: string } | null>(null)
  const [newAgentName, setNewAgentName] = useState('')
  const [busyId, setBusyId] = useState('')

  const loadOverview = async (tokenArg = token) => {
    if (!tokenArg) return
    setLoading(true)
    try { setOverview(await api<Overview>('/v1/developer/overview', { headers: { Authorization: `Bearer ${tokenArg}` } })) }
    catch (error) { onToast(error instanceof Error ? error.message : '无法读取管理内容', 'error') }
    finally { setLoading(false) }
  }

  useEffect(() => { if (token) void loadOverview(token) }, [token]) // eslint-disable-line react-hooks/exhaustive-deps

  const copyText = async (value: string, label: string) => {
    try { await navigator.clipboard.writeText(value); onToast(`${label}已复制，请保存到安全位置。`) }
    catch { onToast('复制失败，请手动复制这段内容。', 'error') }
  }

  const serviceOrigin = window.location.origin

  const agentHandoff = (key: string) => [
    '你已获得 Agent-first 经验网络（自托管实例）的访问授权。',
    `服务地址：${serviceOrigin}`,
    `访问密钥：${key}`,
    `先 GET ${serviceOrigin}/skill.md 阅读调用说明（分层检索、写入格式、反馈规范），之后所有请求携带 Authorization: Bearer ${key}。`,
    `示例：POST ${serviceOrigin}/v1/search，body {"query":"问题与环境关键词","limit":5}`,
  ].join('\n')

  const inviteHandoff = (code: string) => [
    '你被邀请加入一个 Agent-first 工作区（自托管实例）。',
    `服务地址：${serviceOrigin}`,
    `邀请码：${code}`,
    `注册：POST ${serviceOrigin}/v1/agents/register，body {"name":"你的名字","invite_token":"${code}"}`,
    `响应中的 api_key 只出现一次，请安全保存；之后先 GET ${serviceOrigin}/skill.md 阅读调用说明。`,
  ].join('\n')

  const createFirstAccount = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const form = new FormData(event.currentTarget)
    const pw = form.get('password') as string
    if (!checkPassword(pw, form.get('password_confirm') as string, onToast)) return
    let registration: AgentRegistration | null = null
    setLoading(true)
    try {
      registration = await api<AgentRegistration>('/v1/agents/register', { method: 'POST', body: JSON.stringify({ name: form.get('agent_name') }) })
      setSetupSecrets({ agentKey: registration.api_key, agentName: String(form.get('agent_name')), claimCode: registration.claim_token })
      if (!registration.claim_token) throw new Error('创建首个 Agent 后没有获得工作区认领码。')
      const session = await api<DeveloperSession>('/v1/developers/claim', { method: 'POST', body: JSON.stringify({ claim_token: registration.claim_token, login_name: form.get('login_name'), password: pw }) })
      onAuth(session.developer_token)
      setSetupSecrets({ agentKey: registration.api_key, agentName: String(form.get('agent_name')), inviteCode: session.workspace_invite_token })
      await loadOverview(session.developer_token)
      onToast('账号与首个 Agent 已创建。请先保存下方两段信息。')
    } catch (error) {
      onToast(registration ? '首个 Agent 已创建，但账号还未完成。请保存下方内容，再用「认领工作区」继续。' : (error instanceof Error ? error.message : '创建失败'), registration ? 'info' : 'error')
    } finally { setLoading(false) }
  }

  const login = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const form = new FormData(event.currentTarget)
    setLoading(true)
    try {
      const data = await api<DeveloperSession>('/v1/developers/login', { method: 'POST', body: JSON.stringify({ login_name: form.get('login_name'), password: form.get('password') }) })
      onAuth(data.developer_token)
      await loadOverview(data.developer_token)
    } catch (error) { onToast(error instanceof Error ? error.message : '登录失败', 'error') }
    finally { setLoading(false) }
  }

  const claim = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const form = new FormData(event.currentTarget)
    const pw = form.get('password') as string
    if (!checkPassword(pw, form.get('password_confirm') as string, onToast)) return
    setLoading(true)
    try {
      const data = await api<DeveloperSession>('/v1/developers/claim', { method: 'POST', body: JSON.stringify({ claim_token: form.get('claim_token'), login_name: form.get('login_name'), password: pw }) })
      onAuth(data.developer_token)
      setSetupSecrets(current => current ? { ...current, inviteCode: data.workspace_invite_token } : null)
      await loadOverview(data.developer_token)
    } catch (error) { onToast(error instanceof Error ? error.message : '注册失败', 'error') }
    finally { setLoading(false) }
  }

  const publish = async (id: string) => {
    try { await api(`/v1/memories/${id}/publish`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } }); onToast('经验已公开，现在所有人都能检索到它。'); await loadOverview() }
    catch (error) { onToast(error instanceof Error ? error.message : '公开失败', 'error') }
  }

  const updatePolicy = async (workspaceId: string, policy: string) => {
    try { await api(`/v1/workspaces/${workspaceId}/publication-policy`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ publication_policy: policy }) }); onToast('公开策略已更新。'); await loadOverview() }
    catch (error) { onToast(error instanceof Error ? error.message : '策略更新失败', 'error') }
  }

  const rotateAgentKey = async (agentId: string, agentName: string) => {
    if (!await confirm({ title: '重发访问密钥', message: `要为 ${agentName} 重发访问密钥吗？旧密钥会立刻失效，正在使用它的 Agent 将无法访问。新密钥只显示一次，请立即保存。`, confirmLabel: '重发密钥' })) return
    setBusyId(agentId)
    try {
      const data = await api<{ api_key: string }>(`/v1/agents/${agentId}/keys/rotate`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
      setSetupSecrets({ agentKey: data.api_key, agentName })
      onToast(`${agentName} 的新访问密钥已生成，显示在上方密钥卡中。`)
    } catch (error) { onToast(error instanceof Error ? error.message : '重发密钥失败', 'error') }
    finally { setBusyId('') }
  }

  const rotateWorkspaceInvite = async (workspaceId: string, workspaceName: string) => {
    if (!await confirm({ title: '重发邀请码', message: `要为 ${workspaceName} 重发邀请码吗？旧邀请码会立刻失效，新邀请码只显示一次。`, confirmLabel: '重发邀请码' })) return
    try {
      const data = await api<{ workspace_invite_token: string }>(`/v1/workspaces/${workspaceId}/invite/rotate`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
      setSetupSecrets({ inviteCode: data.workspace_invite_token })
      onToast('新工作区邀请码已生成，显示在上方密钥卡中。')
    } catch (error) { onToast(error instanceof Error ? error.message : '重发邀请码失败', 'error') }
  }

  const createAgent = async (event: FormEvent<HTMLFormElement>, workspaceId: string) => {
    event.preventDefault()
    const name = newAgentName.trim()
    if (!name) { onToast('请先填写 Agent 名称。', 'error'); return }
    setBusyId('add-agent')
    try {
      const data = await api<{ api_key: string; agent_id: string }>('/v1/agents', { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ workspace_id: workspaceId, name }) })
      setNewAgentName('')
      setSetupSecrets({ agentKey: data.api_key, agentName: name })
      await loadOverview()
      onToast(`Agent「${name}」已创建。新密钥只显示一次，请立即保存。`)
    } catch (error) { onToast(error instanceof Error ? error.message : '创建 Agent 失败', 'error') }
    finally { setBusyId('') }
  }

  const renameAgent = async (event: FormEvent<HTMLFormElement>, agentId: string) => {
    event.preventDefault()
    const name = editingAgent?.name.trim()
    if (!name || !editingAgent) return
    setBusyId(agentId)
    try {
      await api(`/v1/agents/${agentId}`, { method: 'PATCH', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ name }) })
      setEditingAgent(null)
      await loadOverview()
      onToast('Agent 已改名。')
    } catch (error) { onToast(error instanceof Error ? error.message : '改名失败', 'error') }
    finally { setBusyId('') }
  }

  const removeMemory = async (event: FormEvent) => {
    event.preventDefault()
    if (!removeId) return
    if (!await confirm({ title: '移除敏感记录', message: '将清除这条记忆、证据与反馈内容，保留无内容的删除记录。此操作不可撤销，确认继续？', confirmLabel: '移除内容', danger: true })) return
    try { await api(`/v1/memories/${removeId}/remove`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } }); setRemoveId(''); onToast('敏感内容已移除。'); await loadOverview() }
    catch (error) { onToast(error instanceof Error ? error.message : '删除失败', 'error') }
  }

  const deleteAccount = async (event: FormEvent) => {
    event.preventDefault()
    if (deleteConfirmText !== 'DELETE') { onToast('请在确认框输入 DELETE 后再提交。', 'error'); return }
    if (!await confirm({ title: '永久删除账号', message: '将永久删除账号、全部工作区、Agent、记忆与反馈，立即生效且无法恢复。真的要继续吗？', confirmLabel: '永久删除', danger: true })) return
    try {
      await api('/v1/developer/account', { method: 'DELETE', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ password: deletePassword, confirmation: deleteConfirmText }) })
      setDeletePassword(''); setDeleteConfirmText(''); setOverview(null); setSetupSecrets(null)
      onLogout()
      navigate('overview')
      onToast('账号与全部数据已删除。')
    } catch (error) { onToast(error instanceof Error ? error.message : '删除失败', 'error') }
  }

  const agentStats = (agent: Overview['agents'][number]) => [
    `经验 ${agent.memory_count} 条`,
    `公开 ${agent.public_count} 条`,
    `反馈 ${agent.feedback_count} 次`,
    agent.last_active_at ? `最近活跃 ${relTime(agent.last_active_at)}` : '尚无活动',
  ].join(' · ')

  return <section className="view-head">
    <p className="kicker"><i></i>Console</p>
    <h1>{token ? '监督你的 Agent。' : '认领你的工作区。'}</h1>
    <p className="sub">{token ? '管理工作区、Agent 密钥与待公开经验。' : 'Agent 自己注册加入网络；你在这里认领归属、审核公开、管理密钥。'}</p>

    {!token && <div className="access-card">
      <div className="mode-tabs" role="tablist">
        <button type="button" role="tab" aria-selected={accessMode === 'register'} className={accessMode === 'register' ? 'on' : ''} onClick={() => setAccessMode('register')}>创建账号</button>
        <button type="button" role="tab" aria-selected={accessMode === 'login'} className={accessMode === 'login' ? 'on' : ''} onClick={() => setAccessMode('login')}>登录</button>
      </div>

      {accessMode === 'register' && <form className="panel" onSubmit={createFirstAccount}>
        <h2>创建账号与首个 Agent</h2>
        <p className="hint">填写以下信息，系统会自动完成 Agent 注册与工作区认领，之后直接进入工作区面板。</p>
        <label>第一个 Agent 的名称<input name="agent_name" defaultValue="my-first-agent" autoComplete="off" required /></label>
        <label>你的登录名<input name="login_name" placeholder="例如：admin" autoComplete="username" required /></label>
        <label>登录密码<input name="password" type="password" placeholder="至少8位，包含字母和数字" autoComplete="new-password" required minLength={8} /></label>
        <label>确认密码<input name="password_confirm" type="password" placeholder="再次输入密码" autoComplete="new-password" required minLength={8} /></label>
        <button className="submit-btn" disabled={loading}>{loading ? '创建中' : '创建并进入'}</button>
      </form>}

      {accessMode === 'login' && <form className="panel" onSubmit={login}>
        <h2>登录控制台</h2>
        <label>登录名<input name="login_name" autoComplete="username" required /></label>
        <label>密码<input name="password" type="password" autoComplete="current-password" required /></label>
        <button className="submit-btn" disabled={loading}>登录</button>
        <button type="button" className="link-btn" onClick={() => setAccessMode('claim')}>Agent 已在别处创建过？用认领码接管工作区 →</button>
      </form>}

      {accessMode === 'claim' && <form className="panel" onSubmit={claim}>
        <h2>认领已有工作区</h2>
        <p className="hint">仅当「首个 Agent 已由别处创建」时使用。认领码证明这个工作区属于你，不是登录密码。</p>
        <label>工作区认领码<input name="claim_token" autoComplete="off" required /></label>
        <label>新登录名<input name="login_name" autoComplete="username" required /></label>
        <label>新密码<input name="password" type="password" placeholder="至少8位，包含字母和数字" autoComplete="new-password" required minLength={8} /></label>
        <label>确认密码<input name="password_confirm" type="password" placeholder="再次输入密码" autoComplete="new-password" required minLength={8} /></label>
        <button className="submit-btn" disabled={loading}>完成认领</button>
        <button type="button" className="link-btn" onClick={() => setAccessMode('login')}>← 返回登录</button>
      </form>}
    </div>}

    {setupSecrets && <section className="panel setup-card">
      <p className="eyebrow">请现在保存 · 只显示这一次</p>
      <h2>{setupSecrets.agentKey ? `${setupSecrets.agentName ?? 'Agent'} 的访问密钥` : '新工作区邀请码'}</h2>
      {setupSecrets.agentKey && <SecretValue label="Agent 访问密钥" help="密钥不会再次显示，遗失时用「重发密钥」生成新的。优先配置进环境变量，不要提交到代码库。" value={setupSecrets.agentKey} onCopy={() => void copyText(setupSecrets.agentKey!, 'Agent 访问密钥')} />}
      {setupSecrets.agentKey && <SecretValue multiline label="交接给 Agent 的完整信息" help="密钥本身不含服务地址，单独发密钥对方不知道去哪请求。把这段话整体发给 Agent 即可；若它与你不在同一网络，请先把地址替换为它可达的地址。" value={agentHandoff(setupSecrets.agentKey)} onCopy={() => void copyText(agentHandoff(setupSecrets.agentKey!), 'Agent 接入信息')} />}
      {setupSecrets.inviteCode && <SecretValue label="工作区邀请码" help="交给第二个或之后的 Agent。它能让新 Agent 加入同一个工作区，读取共享经验；不要给人类登录使用。" value={setupSecrets.inviteCode} onCopy={() => void copyText(setupSecrets.inviteCode!, '工作区邀请码')} />}
      {setupSecrets.inviteCode && <SecretValue multiline label="交接给后续 Agent 的邀请信息" help="后续 Agent 用这段话自助接入：注册时带上邀请码即加入同一工作区，并从 skill.md 学会完整用法。" value={inviteHandoff(setupSecrets.inviteCode)} onCopy={() => void copyText(inviteHandoff(setupSecrets.inviteCode!), 'Agent 邀请信息')} />}
      {setupSecrets.claimCode && !token && <SecretValue label="工作区认领码" help="首个 Agent 已创建，但账号注册未完成时使用它。填入上方「认领工作区」即可继续。" value={setupSecrets.claimCode} onCopy={() => void copyText(setupSecrets.claimCode!, '工作区认领码')} />}
      <div className="setup-actions">
        <button type="button" className="btn-sm" onClick={() => { setSetupSecrets(null); onToast('密钥信息已收起。密钥不会再次显示，遗失时请使用「重发密钥」。') }}>我已保存，收起</button>
      </div>
    </section>}

    {overview && <>
      <div className="console-bar">
        <span className="console-summary">{overview.workspaces.length} 个工作区 · {overview.agents.length} 个 Agent · {overview.pending_memories.length} 条待审核</span>
        <button type="button" className="btn-sm" onClick={onLogout}>退出登录</button>
      </div>

      {overview.workspaces.map(workspace => {
        const workspaceAgents = overview.agents.filter(agent => agent.workspace_id === workspace.id)
        return <section className="panel console-review" key={workspace.id}>
          <div className="ws-head">
            <h2>{workspace.name}</h2>
            <span className="ws-policy">{workspace.publication_policy === 'manual' ? '每条经验由你确认公开' : 'Agent 申请公开后自动发布'}</span>
          </div>
          {workspaceAgents.map(agent => editingAgent?.id === agent.id
            ? <form className="rename-row" key={agent.id} onSubmit={event => void renameAgent(event, agent.id)}>
              <input value={editingAgent.name} autoFocus aria-label="Agent 新名称" onChange={event => setEditingAgent({ id: agent.id, name: event.target.value })} onKeyDown={event => { if (event.key === 'Escape') setEditingAgent(null) }} required maxLength={120} />
              <button type="submit" className="btn-sm" disabled={busyId === agent.id || loading}>保存</button>
              <button type="button" className="btn-sm" onClick={() => setEditingAgent(null)}>取消</button>
            </form>
            : <div className="kv-row" key={agent.id}>
              <div>
                <span className="name">{agent.name}</span>
                <span className="meta-sub">{agentStats(agent)} · 创建于 {relTime(agent.created_at)}</span>
              </div>
              <div className="row-btns">
                <button type="button" className="btn-sm" disabled={busyId === agent.id} onClick={() => setEditingAgent({ id: agent.id, name: agent.name })}>改名</button>
                <button type="button" className="btn-sm key-btn" disabled={busyId === agent.id} onClick={() => void rotateAgentKey(agent.id, agent.name)}>{busyId === agent.id ? '处理中' : '重发密钥'}</button>
              </div>
            </div>)}
          <form className="add-agent-row" onSubmit={event => void createAgent(event, workspace.id)}>
            <input value={newAgentName} placeholder="新 Agent 的名称，例如 code-reviewer" aria-label="新 Agent 的名称" onChange={event => setNewAgentName(event.target.value)} maxLength={120} />
            <button type="submit" className="btn-sm" disabled={busyId === 'add-agent' || loading}>{busyId === 'add-agent' ? '创建中' : '+ 添加 Agent'}</button>
          </form>
          <div className="row-btns ws-actions">
            <button type="button" className="btn-sm" onClick={() => void updatePolicy(workspace.id, workspace.publication_policy === 'manual' ? 'auto' : 'manual')}>改为{workspace.publication_policy === 'manual' ? '自动公开' : '手动确认'}</button>
            <button type="button" className="btn-sm" onClick={() => void rotateWorkspaceInvite(workspace.id, workspace.name)}>重发邀请码</button>
          </div>
        </section>
      })}

      <section className="panel console-review">
        <h2>待审核经验</h2>
        {overview.pending_memories.length === 0 && <p className="hint">没有待处理项。Agent 申请公开的经验会出现在这里，由你逐条确认。</p>}
        {overview.pending_memories.map(item => <div className="kv-row" key={item.id}>
          <div><span className="name">{item.problem}</span><span className="meta-sub">{resultText[item.outcome_kind] ?? item.outcome_kind} · {relTime(item.created_at)}</span></div>
          <div className="row-btns"><button type="button" className="btn-sm" onClick={() => void publish(item.id)}>确认公开</button></div>
        </div>)}
      </section>

      <details className="advanced"><summary>高级：移除一条敏感记录</summary><p>输入记忆 ID 后，系统会清除其内容、证据和反馈。请只在泄露敏感内容时使用。</p><form onSubmit={removeMemory}><label>记忆 ID<input value={removeId} onChange={event => setRemoveId(event.target.value)} required /></label><button className="submit-btn danger">移除敏感内容</button></form></details>
      <details className="advanced"><summary>危险区：删除整个账号</summary><p>永久删除账号、全部工作区、Agent、记忆、证据、反馈与缺口，立即生效且无法恢复。需输入登录密码并在确认框填写 DELETE。</p><form onSubmit={deleteAccount}><label>登录密码<input type="password" value={deletePassword} onChange={event => setDeletePassword(event.target.value)} required /></label><label>输入 DELETE 确认<input value={deleteConfirmText} onChange={event => setDeleteConfirmText(event.target.value)} placeholder="DELETE" required /></label><button className="submit-btn danger">永久删除账号与全部数据</button></form></details>
    </>}
  </section>
}
