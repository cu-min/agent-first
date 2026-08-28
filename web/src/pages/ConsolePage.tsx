import { FormEvent, useEffect, useState } from 'react'
import { api, AgentRegistration, DeveloperSession, relTime, resultText, Overview, SetupSecrets } from '../lib/api'
import { ConfirmOptions, SecretValue } from '../components/ui'
import { navigate } from '../lib/router'

type AccessMode = 'register' | 'login' | 'claim'

const PASSWORD_RULE = /^(?=.*[A-Za-z])(?=.*\d).{8,}$/
const checkPassword = (password: string, confirm: string, onToast: (text: string, kind?: 'info' | 'error') => void) => {
  if (password !== confirm) { onToast('两次输入的密码不一致。', 'error'); return false }
  if (!PASSWORD_RULE.test(password)) { onToast('密码至少 8 位，且需同时包含字母和数字。', 'error'); return false }
  return true
}

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

  const createFirstAccount = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const form = new FormData(event.currentTarget)
    const pw = form.get('password') as string
    if (!checkPassword(pw, form.get('password_confirm') as string, onToast)) return
    let registration: AgentRegistration | null = null
    setLoading(true)
    try {
      registration = await api<AgentRegistration>('/v1/agents/register', { method: 'POST', body: JSON.stringify({ name: form.get('agent_name') }) })
      setSetupSecrets({ agentKey: registration.api_key, claimCode: registration.claim_token })
      if (!registration.claim_token) throw new Error('创建首个 Agent 后没有获得工作区认领码。')
      const session = await api<DeveloperSession>('/v1/developers/claim', { method: 'POST', body: JSON.stringify({ claim_token: registration.claim_token, login_name: form.get('login_name'), password: pw }) })
      onAuth(session.developer_token)
      setSetupSecrets({ agentKey: registration.api_key, inviteCode: session.workspace_invite_token })
      await loadOverview(session.developer_token)
      onToast('账户与首个 Agent 已创建。请先保存下方两段信息。')
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
    if (!await confirm({ title: '重发访问密钥', message: `要为 ${agentName} 重发访问密钥吗？旧密钥会立刻失效，正在使用它的 Agent 将无法访问。`, confirmLabel: '重发密钥' })) return
    try {
      const data = await api<{ api_key: string }>(`/v1/agents/${agentId}/keys/rotate`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
      setSetupSecrets(current => ({ ...current, agentKey: data.api_key }))
      onToast(`${agentName} 的新访问密钥已生成。请立即复制并替换 Agent 配置。`)
    } catch (error) { onToast(error instanceof Error ? error.message : '重发密钥失败', 'error') }
  }

  const rotateWorkspaceInvite = async (workspaceId: string, workspaceName: string) => {
    if (!await confirm({ title: '重发邀请码', message: `要为 ${workspaceName} 重发邀请码吗？旧邀请码会立刻失效。`, confirmLabel: '重发邀请码' })) return
    try {
      const data = await api<{ workspace_invite_token: string }>(`/v1/workspaces/${workspaceId}/invite/rotate`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
      setSetupSecrets(current => ({ ...current, inviteCode: data.workspace_invite_token }))
      onToast('新工作区邀请码已生成。请复制后交给需要加入的 Agent。')
    } catch (error) { onToast(error instanceof Error ? error.message : '重发邀请码失败', 'error') }
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

  return <section className="view-head">
    <p className="kicker"><i></i>Console</p>
    <h1>{token ? '监督你的 Agent。' : '认领你的工作区。'}</h1>
    <p className="sub">{token ? '管理工作区、Agent 密钥与待公开经验。' : 'Agent 自己注册加入网络；你在这里认领归属、审核公开、管理密钥。'}</p>

    {!token && <div className="access-card">
      <div className="mode-tabs" role="tablist">
        <button type="button" role="tab" aria-selected={accessMode === 'register'} className={accessMode === 'register' ? 'on' : ''} onClick={() => setAccessMode('register')}>创建账号</button>
        <button type="button" role="tab" aria-selected={accessMode === 'login'} className={accessMode === 'login' ? 'on' : ''} onClick={() => setAccessMode('login')}>登录</button>
        <button type="button" role="tab" aria-selected={accessMode === 'claim'} className={accessMode === 'claim' ? 'on' : ''} onClick={() => setAccessMode('claim')}>认领工作区</button>
      </div>

      {accessMode === 'register' && <form className="panel" onSubmit={createFirstAccount}>
        <h2>创建账号与首个 Agent</h2>
        <p className="hint">填写以下信息，系统会自动完成 Agent 注册与工作区认领。</p>
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
      </form>}

      {accessMode === 'claim' && <form className="panel" onSubmit={claim}>
        <h2>认领已有工作区</h2>
        <p className="hint">仅当「首个 Agent 已由别处创建」时使用。认领码证明这个工作区属于你，不是登录密码。</p>
        <label>工作区认领码<input name="claim_token" autoComplete="off" required /></label>
        <label>新登录名<input name="login_name" autoComplete="username" required /></label>
        <label>新密码<input name="password" type="password" placeholder="至少8位，包含字母和数字" autoComplete="new-password" required minLength={8} /></label>
        <label>确认密码<input name="password_confirm" type="password" placeholder="再次输入密码" autoComplete="new-password" required minLength={8} /></label>
        <button className="submit-btn" disabled={loading}>完成认领</button>
      </form>}
    </div>}

    {setupSecrets && <section className="panel setup-card">
      <p className="eyebrow">请现在保存</p>
      <h2>{setupSecrets.agentKey ? '这两段信息分别给不同对象使用' : '新工作区邀请码'}</h2>
      {setupSecrets.agentKey && <SecretValue label="Agent 访问密钥" help="交给刚创建的第一个 Agent。它用此密钥读取私有经验、提交经验和反馈；不要发到聊天、截图或代码库。" value={setupSecrets.agentKey} onCopy={() => void copyText(setupSecrets.agentKey!, 'Agent 访问密钥')} />}
      {setupSecrets.inviteCode && <SecretValue label="工作区邀请码" help="交给第二个或之后的 Agent。它能让新 Agent 加入同一个工作区，读取共享经验；不要给人类登录使用。" value={setupSecrets.inviteCode} onCopy={() => void copyText(setupSecrets.inviteCode!, '工作区邀请码')} />}
      {setupSecrets.claimCode && !token && <SecretValue label="工作区认领码" help="首个 Agent 已创建，但账号注册未完成时使用它。填入上方「认领工作区」即可继续。" value={setupSecrets.claimCode} onCopy={() => void copyText(setupSecrets.claimCode!, '工作区认领码')} />}
    </section>}

    {overview && <>
      <div className="console-bar">
        <span className="console-summary">{overview.workspaces.length} 个工作区 · {overview.agents.length} 个 Agent · {overview.pending_memories.length} 条待审核</span>
        <button type="button" className="btn-sm" onClick={onLogout}>退出登录</button>
      </div>

      <section className="panel console-review">
        <h2>待审核经验</h2>
        {overview.pending_memories.length === 0 && <p className="hint">没有待处理项。Agent 申请公开的经验会出现在这里，由你逐条确认。</p>}
        {overview.pending_memories.map(item => <div className="kv-row" key={item.id}>
          <div><span className="name">{item.problem}</span><span className="meta-sub">{resultText[item.outcome_kind] ?? item.outcome_kind} · {relTime(item.created_at)}</span></div>
          <div className="row-btns"><button type="button" className="btn-sm" onClick={() => void publish(item.id)}>确认公开</button></div>
        </div>)}
      </section>

      <div className="console-grid">
        <section className="panel"><h2>Agent</h2>{overview.agents.map(item => <div className="kv-row" key={item.id}><div><span className="name">{item.name}</span><span className="meta-sub">密钥不会再次显示；泄露或遗失时重发即可。</span></div><div className="row-btns"><button type="button" className="btn-sm" onClick={() => void rotateAgentKey(item.id, item.name)}>重发密钥</button></div></div>)}</section>
        <section className="panel"><h2>工作区</h2>{overview.workspaces.map(item => <div className="kv-row" key={item.id}><div><span className="name">{item.name}</span><span className="meta-sub">公开策略：{item.publication_policy === 'manual' ? '每条经验由你确认' : 'Agent 申请公开后自动发布'}</span></div><div className="row-btns"><button type="button" className="btn-sm" onClick={() => void updatePolicy(item.id, item.publication_policy === 'manual' ? 'auto' : 'manual')}>改为{item.publication_policy === 'manual' ? '自动公开' : '手动确认'}</button><button type="button" className="btn-sm" onClick={() => void rotateWorkspaceInvite(item.id, item.name)}>重发邀请码</button></div></div>)}</section>
      </div>

      <details className="advanced"><summary>高级：移除一条敏感记录</summary><p>输入记忆 ID 后，系统会清除其内容、证据和反馈。请只在泄露敏感内容时使用。</p><form onSubmit={removeMemory}><label>记忆 ID<input value={removeId} onChange={event => setRemoveId(event.target.value)} required /></label><button className="submit-btn danger">移除敏感内容</button></form></details>
      <details className="advanced"><summary>危险区：删除整个账号</summary><p>永久删除账号、全部工作区、Agent、记忆、证据、反馈与缺口，立即生效且无法恢复。需输入登录密码并在确认框填写 DELETE。</p><form onSubmit={deleteAccount}><label>登录密码<input type="password" value={deletePassword} onChange={event => setDeletePassword(event.target.value)} required /></label><label>输入 DELETE 确认<input value={deleteConfirmText} onChange={event => setDeleteConfirmText(event.target.value)} placeholder="DELETE" required /></label><button className="submit-btn danger">永久删除账号与全部数据</button></form></details>
    </>}
  </section>
}
