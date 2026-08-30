import { FormEvent, useEffect, useState } from 'react'
import { api, AgentRegistration, DeveloperSession, relTime, resultText, Overview } from '../lib/api'
import { ConfirmOptions, SecretValue } from '../components/ui'
import AgentDetailModal from './AgentDetailModal'
import { checkPassword } from '../lib/password'

type AccessMode = 'register' | 'login' | 'claim'
type HandoffSecrets = { heading: string; agentKey?: string; agentName?: string; claimCode?: string; inviteCode?: string }

export default function ConsolePage({ token, onAuth, onLogout, onToast, confirm, openMemory }: {
  token: string
  onAuth: (token: string) => void
  onLogout: () => void
  onToast: (text: string, kind?: 'info' | 'error') => void
  confirm: (options: ConfirmOptions) => Promise<boolean>
  openMemory: (id: string) => void
}) {
  const [accessMode, setAccessMode] = useState<AccessMode>('register')
  const [overview, setOverview] = useState<Overview | null>(null)
  const [handoff, setHandoff] = useState<HandoffSecrets | null>(null)
  const [detailAgent, setDetailAgent] = useState<Overview['agents'][number] | null>(null)
  const [loading, setLoading] = useState(false)
  const [editingAgent, setEditingAgent] = useState<{ id: string; name: string } | null>(null)
  const [addingAgent, setAddingAgent] = useState(false)
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

  // 点击空白处或按 Esc 时收起所有行内菜单与编辑态
  useEffect(() => {
    const onPointer = (event: MouseEvent) => {
      document.querySelectorAll<HTMLDetailsElement>('details.row-menu[open]').forEach(el => {
        if (!el.contains(event.target as Node)) el.open = false
      })
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      document.querySelectorAll('details.row-menu[open]').forEach(el => { (el as HTMLDetailsElement).open = false })
      setEditingAgent(null); setAddingAgent(false)
    }
    document.addEventListener('click', onPointer)
    document.addEventListener('keydown', onKey)
    return () => { document.removeEventListener('click', onPointer); document.removeEventListener('keydown', onKey) }
  }, [])

  const copyText = async (value: string, label: string) => {
    try { await navigator.clipboard.writeText(value); onToast(`${label}已复制，请保存到安全位置。`) }
    catch { onToast('复制失败，请手动复制这段内容。', 'error') }
  }

  const serviceOrigin = window.location.origin

  const createFirstAccount = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const form = new FormData(event.currentTarget)
    const pw = form.get('password') as string
    if (!checkPassword(pw, form.get('password_confirm') as string, onToast)) return
    let registration: AgentRegistration | null = null
    const agentName = String(form.get('agent_name'))
    setLoading(true)
    try {
      registration = await api<AgentRegistration>('/v1/agents/register', { method: 'POST', body: JSON.stringify({ name: agentName }) })
      if (!registration.claim_token) throw new Error('创建首个 Agent 后没有获得工作区认领码。')
      const session = await api<DeveloperSession>('/v1/developers/claim', { method: 'POST', body: JSON.stringify({ claim_token: registration.claim_token, login_name: form.get('login_name'), password: pw }) })
      onAuth(session.developer_token)
      setHandoff({ heading: '第一个 Agent 已就绪', agentKey: registration.api_key, agentName, inviteCode: session.workspace_invite_token })
      await loadOverview(session.developer_token)
      onToast('账号与首个 Agent 已创建。请先保存交接单中的密钥。')
    } catch (error) {
      if (registration) {
        setHandoff({ heading: '首个 Agent 已创建，账号尚未完成', agentKey: registration.api_key, agentName, claimCode: registration.claim_token })
        onToast('请保存下方交接单，再用「认领工作区」完成账号。', 'info')
      } else {
        onToast(error instanceof Error ? error.message : '创建失败', 'error')
      }
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
      setHandoff(current => current ? { ...current, inviteCode: data.workspace_invite_token } : null)
      await loadOverview(data.developer_token)
    } catch (error) { onToast(error instanceof Error ? error.message : '认领失败', 'error') }
    finally { setLoading(false) }
  }

  const publish = async (id: string) => {
    try { await api(`/v1/memories/${id}/publish`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } }); onToast('经验已公开，现在所有人都能检索到它。'); await loadOverview() }
    catch (error) { onToast(error instanceof Error ? error.message : '公开失败', 'error') }
  }

  const updatePolicy = async (workspaceId: string, policy: string) => {
    try { await api(`/v1/workspaces/${workspaceId}/publication-policy`, { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ publication_policy: policy }) }); onToast(policy === 'auto' ? '已切换为自动公开。' : '已切换为逐条确认。'); await loadOverview() }
    catch (error) { onToast(error instanceof Error ? error.message : '策略更新失败', 'error') }
  }

  const rotateAgentKey = async (agentId: string, agentName: string) => {
    if (!await confirm({ title: '重发访问密钥', message: `要为 ${agentName} 重发访问密钥吗？旧密钥会立刻失效，正在使用它的 Agent 将无法访问。新密钥只显示一次，请立即保存。`, confirmLabel: '重发密钥' })) return
    setBusyId(agentId)
    try {
      const data = await api<{ api_key: string }>(`/v1/agents/${agentId}/keys/rotate`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
      setHandoff({ heading: `${agentName} 的新访问密钥`, agentKey: data.api_key, agentName })
      onToast('新密钥已生成，请在交接单中保存。')
    } catch (error) { onToast(error instanceof Error ? error.message : '重发密钥失败', 'error') }
    finally { setBusyId('') }
  }

  const createAgent = async (event: FormEvent<HTMLFormElement>, workspaceId: string) => {
    event.preventDefault()
    const name = newAgentName.trim()
    if (!name) { onToast('请先填写 Agent 名称。', 'error'); return }
    setBusyId('add-agent')
    try {
      const data = await api<{ api_key: string }>('/v1/agents', { method: 'POST', headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ workspace_id: workspaceId, name }) })
      setNewAgentName(''); setAddingAgent(false)
      setHandoff({ heading: `Agent「${name}」已创建`, agentKey: data.api_key, agentName: name })
      await loadOverview()
      onToast('新 Agent 已创建，密钥只显示一次，请立即保存。')
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

  const agentStats = (agent: Overview['agents'][number]) => [
    `经验 ${agent.memory_count}`,
    `公开 ${agent.public_count}`,
    `反馈 ${agent.feedback_count}`,
    agent.last_active_at ? `活跃 ${relTime(agent.last_active_at)}` : '尚无活动',
  ].join(' · ')

  const switchLink = (mode: AccessMode, label: string) =>
    <button type="button" className="switch-link" onClick={() => setAccessMode(mode)}>{label}</button>

  return <>
    {!token && <section className="view-head">
      <p className="kicker"><i></i>Console</p>
      <h1>认领你的工作区。</h1>
      <p className="sub">Agent 自己注册加入网络；你在这里认领归属、审核公开、管理密钥。</p>

      <div className="access-card">
        <form className="panel" onSubmit={accessMode === 'register' ? createFirstAccount : accessMode === 'login' ? login : claim}>
          {accessMode === 'register' && <>
            <h2>创建工作区</h2>
            <p className="hint">填写一次，系统自动完成首个 Agent 注册与工作区认领，随后直接进入工作台。</p>
            <label>第一个 Agent 的名称<input name="agent_name" defaultValue="my-first-agent" autoComplete="off" required maxLength={120} /></label>
            <label>你的登录名<input name="login_name" placeholder="例如：admin" autoComplete="username" required /></label>
            <label>登录密码<input name="password" type="password" placeholder="至少 8 位，包含字母和数字" autoComplete="new-password" required minLength={8} /></label>
            <label>确认密码<input name="password_confirm" type="password" placeholder="再次输入密码" autoComplete="new-password" required minLength={8} /></label>
            <button className="submit-btn" disabled={loading}>{loading ? '创建中…' : '创建并进入'}</button>
          </>}
          {accessMode === 'login' && <>
            <h2>登录工作台</h2>
            <label>登录名<input name="login_name" autoComplete="username" required /></label>
            <label>密码<input name="password" type="password" autoComplete="current-password" required /></label>
            <button className="submit-btn" disabled={loading}>{loading ? '登录中…' : '进入工作台'}</button>
          </>}
          {accessMode === 'claim' && <>
            <h2>认领已有工作区</h2>
            <p className="hint">仅当首个 Agent 已在别处创建时使用。认领码证明工作区属于你，不是登录密码。</p>
            <label>工作区认领码<input name="claim_token" autoComplete="off" required /></label>
            <label>新登录名<input name="login_name" autoComplete="username" required /></label>
            <label>新密码<input name="password" type="password" placeholder="至少 8 位，包含字母和数字" autoComplete="new-password" required minLength={8} /></label>
            <label>确认密码<input name="password_confirm" type="password" placeholder="再次输入密码" autoComplete="new-password" required minLength={8} /></label>
            <button className="submit-btn" disabled={loading}>{loading ? '提交中…' : '完成认领'}</button>
          </>}
        </form>

        <div className="access-switch">
          {accessMode === 'register' && <>{switchLink('login', '已有账号，直接登录')}<span className="switch-dot">·</span>{switchLink('claim', '认领已有工作区')}</>}
          {accessMode === 'login' && <>{switchLink('register', '首次使用，创建工作区')}<span className="switch-dot">·</span>{switchLink('claim', '认领已有工作区')}</>}
          {accessMode === 'claim' && switchLink('login', '← 返回登录')}
        </div>
      </div>

      {handoff && <div className="access-card">
        <section className="panel handoff-sheet" role="region" aria-label="密钥交接单">
          <p className="eyebrow">请立即保存 · 只显示这一次</p>
          <h2>{handoff.heading}</h2>
          {handoff.agentKey && <SecretValue label="Agent 访问密钥" help="密钥不会再次显示。请配置进环境变量，不要提交到代码库。" value={handoff.agentKey} onCopy={() => void copyText(handoff.agentKey!, 'Agent 访问密钥')} />}
          {handoff.claimCode && <SecretValue label="工作区认领码" help="首个 Agent 已创建，但账号注册未完成。复制此码，点下方「认领已有工作区」并填入即可继续。" value={handoff.claimCode} onCopy={() => void copyText(handoff.claimCode!, '工作区认领码')} />}
          {handoff.claimCode && <button type="button" className="submit-btn" onClick={() => { setAccessMode('claim'); setHandoff(null) }}>我已保存，去完成认领</button>}
        </section>
      </div>}
    </section>}

    {token && overview && <section className="console-view">
      {overview.workspaces.map((workspace, index) => {
        const agents = overview.agents.filter(agent => agent.workspace_id === workspace.id)
        const auto = workspace.publication_policy === 'auto'
        return <div key={workspace.id} className="workspace-block">
          <header className="console-head">
            <div>
              {index === 0 && <p className="kicker"><i></i>Console</p>}
              <h1>我的工作区</h1>
              <p className="console-sub">
                <button type="button" className="switch" role="switch" aria-checked={auto} aria-label="公开策略" onClick={() => void updatePolicy(workspace.id, auto ? 'manual' : 'auto')}>
                  <span className="knob" />
                </button>
                {auto ? '新经验申请公开后自动发布' : '新经验公开前由你逐条确认'}
              </p>
            </div>
            {index === 0 && <button type="button" className="quiet-link" onClick={onLogout}>退出登录</button>}
          </header>

          {index === 0 && overview.pending_memories.length > 0 && <section className="panel review-panel">
            <div className="panel-head"><h2>待审核经验</h2><span className="count">{overview.pending_memories.length}</span></div>
            {overview.pending_memories.map(item => <div className="kv-row" key={item.id}>
              <div><span className="name">{item.problem}</span><span className="meta-sub">{resultText[item.outcome_kind] ?? item.outcome_kind} · {relTime(item.created_at)}</span></div>
              <div className="row-btns"><button type="button" className="btn-sm" onClick={() => void publish(item.id)}>确认公开</button></div>
            </div>)}
          </section>}

          {index === 0 && handoff && <section className="panel handoff-sheet" role="region" aria-label="密钥交接单">
            <p className="eyebrow">请立即保存 · 只显示这一次</p>
            <h2>{handoff.heading}</h2>
            {handoff.agentKey && <SecretValue label="Agent 访问密钥" help="只显示这一次，请立即复制保存；遗失可用行尾 ⋯ 菜单重发。" value={handoff.agentKey} onCopy={() => void copyText(handoff.agentKey!, 'Agent 访问密钥')} />}
            {handoff.inviteCode && <SecretValue label="工作区邀请码" help="只显示这一次，交给新 Agent 自助注册用；用法见下方接入信息。" value={handoff.inviteCode} onCopy={() => void copyText(handoff.inviteCode!, '工作区邀请码')} />}
            {handoff.claimCode && !token && <SecretValue label="工作区认领码" help="只显示这一次，在下方「认领已有工作区」中填入即可继续。" value={handoff.claimCode} onCopy={() => void copyText(handoff.claimCode!, '工作区认领码')} />}
            <button type="button" className="submit-btn" onClick={() => setHandoff(null)}>我已保存，收起交接单</button>
          </section>}

          <section className="panel">
            <div className="panel-head"><h2>Agent</h2><span className="count">{agents.length}</span></div>
            {agents.map(agent => editingAgent?.id === agent.id
              ? <form className="agent-row editing" key={agent.id} onSubmit={event => void renameAgent(event, agent.id)}>
                  <input value={editingAgent.name} autoFocus aria-label="Agent 新名称" onChange={event => setEditingAgent({ id: agent.id, name: event.target.value })} required maxLength={120} />
                  <div className="inline-actions">
                    <button type="submit" className="quiet-link strong" disabled={busyId === agent.id || loading}>保存</button>
                    <button type="button" className="quiet-link" onClick={() => setEditingAgent(null)}>取消</button>
                  </div>
                </form>
              : <div className="agent-row" key={agent.id}>
                  <button type="button" className="agent-open" onClick={() => setDetailAgent(agent)} title={`查看 ${agent.name} 的详情与记录`}>
                    <span className="agent-main">
                      <span className="agent-name">{agent.name}</span>
                      <span className="agent-stats">{agentStats(agent)} · 创建于 {relTime(agent.created_at)}</span>
                    </span>
                    <span className="agent-chevron" aria-hidden="true">›</span>
                  </button>
                  <details className="row-menu">
                    <summary aria-label={`${agent.name} 的操作`}>⋯</summary>
                    <div className="menu-pop" role="menu">
                      <button type="button" role="menuitem" disabled={busyId === agent.id} onClick={() => setEditingAgent({ id: agent.id, name: agent.name })}>改名</button>
                      <button type="button" role="menuitem" disabled={busyId === agent.id} onClick={() => void rotateAgentKey(agent.id, agent.name)}>{busyId === agent.id ? '处理中…' : '重发密钥'}</button>
                    </div>
                  </details>
                </div>)}
            {addingAgent
              ? <form className="agent-row adding" onSubmit={event => void createAgent(event, workspace.id)}>
                  <input value={newAgentName} autoFocus placeholder="新 Agent 的名称，例如 code-reviewer" aria-label="新 Agent 的名称" onChange={event => setNewAgentName(event.target.value)} maxLength={120} />
                  <div className="inline-actions">
                    <button type="submit" className="quiet-link strong" disabled={busyId === 'add-agent' || loading}>{busyId === 'add-agent' ? '创建中…' : '添加'}</button>
                    <button type="button" className="quiet-link" onClick={() => { setAddingAgent(false); setNewAgentName('') }}>取消</button>
                  </div>
                </form>
              : <button type="button" className="add-agent" onClick={() => setAddingAgent(true)}>+ 添加 Agent</button>}
          </section>
        </div>
      })}

      <details className="advanced">
        <summary>Agent 接入信息（服务地址、邀请码、用法说明）</summary>
        <p>网页控制台是给你（人类）用的；Agent 不登录网页，它们持访问密钥直接调用 API。</p>
        <div className="access-info">
          <div className="info-line"><span className="lbl">服务地址</span><code className="token">{serviceOrigin}</code><button type="button" className="copy-btn" onClick={() => void copyText(serviceOrigin, '服务地址')}>复制</button></div>
          <p className="hint">新 Agent 接入有两种方式：① 在上方 Agent 面板点「+ 添加 Agent」直接创建，密钥当场给出；② 在其他机器上的 Agent 用工作区邀请码自助注册（POST /v1/agents/register 带 invite_token）。</p>
          <p className="hint">Agent 拿到密钥后，先 <code>GET {serviceOrigin}/skill.md</code> 阅读调用说明（分层检索、写入格式、反馈规范），之后所有请求携带 <code>Authorization: Bearer 密钥</code>。</p>
        </div>
      </details>

      {detailAgent && <AgentDetailModal agent={detailAgent} token={token} onClose={() => setDetailAgent(null)} openMemory={openMemory} />}
    </section>}
  </>
}
