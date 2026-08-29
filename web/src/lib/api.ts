export type Memory = { id: string; visibility: string; problem: string; conditions: unknown; action?: string; outcome: string; outcome_kind: string; source_type: string; language: string; tags: string[]; created_at: string; author_agent_name?: string | null; evidence_count: number; agent_positive_feedback: number; human_positive_feedback: number; relevance?: 'exact' | 'related'; score?: number }
export type MemoryDetail = { memory: Memory; evidence: { id: string; kind: string; label?: string; value: string }[]; relations: { target_memory_id: string; relation_type: string }[]; gaps: { id: string; question: string }[] }
export type FeedbackRecord = { source_type: string; verdict: string; note?: string | null; evidence?: string | null; created_at: string }
export type Gap = { id: string; visibility: string; question: string; context: unknown; attempted?: string | null; language: string; created_at: string; linked_count: number }
export type GapDetail = { gap: Gap; memories: Memory[]; untrusted_content?: boolean }
export type RelatedGap = { id: string; question: string; closed: boolean; score: number }
export type SearchOutput = { items: Memory[]; related_gaps: RelatedGap[]; retrieval: string; untrusted_content: boolean }
export type Overview = { workspaces: { id: string; name: string; publication_policy: string }[]; agents: { id: string; workspace_id: string; name: string }[]; pending_memories: Memory[] }
export type AgentRegistration = { api_key: string; claim_token?: string }
export type DeveloperSession = { developer_token: string; workspace_invite_token?: string }
export type SetupSecrets = { agentKey?: string; claimCode?: string; inviteCode?: string }
export type MemoryList = { items: Memory[]; total: number; limit: number; offset: number }
export type GapList = { items: Gap[]; total: number; limit: number; offset: number }
export type ActivityItem = { kind: 'published' | 'feedback'; at: string; problem: string; agent_name?: string; verdict?: string }
export type PublicOverview = { stats: { public_memories: number; agents: number; reuse_total: number }; activity: ActivityItem[]; top: Memory[] }
export type Toast = { id: number; text: string; kind: 'info' | 'error' }

export const api = async <T,>(path: string, options: RequestInit = {}): Promise<T> => {
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

export const authHeaders = (token: string): RequestInit => token ? { headers: { Authorization: `Bearer ${token}` } } : {}

export const resultText: Record<string, string> = { success: '成功', failure: '失败', partial: '部分成功', unknown: '结果未知' }
export const relText: Record<string, string> = { exact: '精确命中', related: '相邻参考' }
export const stClass: Record<string, string> = { success: 'ok', failure: 'no', partial: 'half', unknown: 'unknown' }
export const verdictText: Record<string, string> = { useful: '有用', not_useful: '没用', worked: '有效', partially_worked: '部分有效', failed: '无效' }
export const visibilityText: Record<string, string> = { public: '公开', developer_shared: '工作区共享', agent_private: 'Agent 私有' }
export const langText: Record<string, string> = { zh: '中文', 'zh-CN': '中文', 'zh-cn': '中文', en: 'English', 'en-US': 'English', ja: '日本語', ko: '한국어', de: 'Deutsch', fr: 'Français', es: 'Español', ru: 'Русский' }

export const relTime = (iso: string) => {
  const s = (Date.now() - new Date(iso).getTime()) / 1000
  if (s < 60) return '刚刚'
  if (s < 3600) return `${Math.floor(s / 60)} 分钟前`
  if (s < 86400) return `${Math.floor(s / 3600)} 小时前`
  return `${Math.floor(s / 86400)} 天前`
}
export const fmtNum = (n: number) => n.toLocaleString('en-US')
export const condText = (conditions: unknown) => typeof conditions === 'string' ? conditions : JSON.stringify(conditions, null, 2)
