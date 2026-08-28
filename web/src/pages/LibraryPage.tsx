import { FormEvent, useEffect, useRef, useState } from 'react'
import { api, authHeaders, Memory, MemoryList } from '../lib/api'
import { MemoryCard, SkeletonCards } from '../components/ui'
import { navigate } from '../lib/router'

type LibraryFilter = 'public' | 'workspace' | 'agent'
type FilterOverrides = { outcome?: string; time?: string; sort?: string }

export default function LibraryPage({ token, onToast, openMemory }: { token: string; onToast: (text: string, kind?: 'info' | 'error') => void; openMemory: (id: string) => void }) {
  const [libraryFilter, setLibraryFilter] = useState<LibraryFilter>('public')
  const [query, setQuery] = useState('')
  const [searchResults, setSearchResults] = useState<Memory[] | null>(null)
  const [searching, setSearching] = useState(false)
  const [publicList, setPublicList] = useState<MemoryList | null>(null)
  const [mineList, setMineList] = useState<MemoryList | null>(null)
  const [loading, setLoading] = useState(false)

  const [filterOutcome, setFilterOutcome] = useState('')
  const [filterTime, setFilterTime] = useState('')
  const [filterSort, setFilterSort] = useState('latest')

  const searchTimer = useRef<number | null>(null)
  const searchSeq = useRef(0)

  const buildQuery = (o: FilterOverrides = {}) => {
    const params = new URLSearchParams()
    const outcome = o.outcome ?? filterOutcome
    const time = o.time ?? filterTime
    const sort = o.sort ?? filterSort
    if (libraryFilter !== 'public') params.set('visibility', libraryFilter === 'workspace' ? 'developer_shared' : 'agent_private')
    if (outcome) params.set('outcome_kind', outcome)
    const now = Date.now()
    if (time === '1d') params.set('since', new Date(now - 86400000).toISOString())
    if (time === '3d') params.set('since', new Date(now - 86400000 * 3).toISOString())
    if (time === '1w') params.set('since', new Date(now - 86400000 * 7).toISOString())
    if (time === '1m') params.set('since', new Date(now - 86400000 * 30).toISOString())
    if (sort && sort !== 'latest') params.set('order_by', sort)
    return params.toString()
  }

  const loadPublicList = async (offset = 0, o: FilterOverrides = {}) => {
    setLoading(true)
    try {
      const qs = buildQuery(o)
      const data = await api<MemoryList>(`/v1/public/memories?limit=20&offset=${offset}${qs ? '&' + qs : ''}`)
      setPublicList(current => offset > 0 && current ? { ...data, items: [...current.items, ...data.items] } : data)
    } catch (error) { onToast(error instanceof Error ? error.message : '无法读取公开经验', 'error') }
    finally { setLoading(false) }
  }

  const loadMineList = async (offset = 0, o: FilterOverrides = {}, tokenArg = token) => {
    if (!tokenArg) return
    setLoading(true)
    try {
      const qs = buildQuery(o)
      const data = await api<MemoryList>(`/v1/memories?limit=20&offset=${offset}${qs ? '&' + qs : ''}`, { headers: { Authorization: `Bearer ${tokenArg}` } })
      setMineList(current => offset > 0 && current ? { ...data, items: [...current.items, ...data.items] } : data)
    } catch (error) { onToast(error instanceof Error ? error.message : '无法读取记忆列表', 'error') }
    finally { setLoading(false) }
  }

  useEffect(() => {
    if (libraryFilter === 'public' && !publicList) void loadPublicList()
  }, [libraryFilter]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (libraryFilter !== 'public' && token && !mineList) void loadMineList(0, {}, token)
    if (!token) setMineList(null)
  }, [libraryFilter, token]) // eslint-disable-line react-hooks/exhaustive-deps

  const doSearch = async (q: string) => {
    if (q.trim().length < 2) { setSearchResults(null); setSearching(false); return }
    const seq = ++searchSeq.current
    setSearching(true)
    try {
      const data = await api<{ items: Memory[] }>('/v1/search', { method: 'POST', body: JSON.stringify({ query: q.trim(), limit: 10 }), ...authHeaders(token) })
      if (seq !== searchSeq.current) return
      setSearchResults(data.items)
    } catch (error) {
      if (seq !== searchSeq.current) return
      onToast(error instanceof Error ? error.message : '检索失败', 'error')
    } finally {
      if (seq === searchSeq.current) setSearching(false)
    }
  }

  const onQueryChange = (value: string) => {
    setQuery(value)
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    if (value.trim().length < 2) { setSearchResults(null); setSearching(false); return }
    searchTimer.current = window.setTimeout(() => void doSearch(value), 250)
  }

  const clearSearch = () => {
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    searchSeq.current++
    setSearchResults(null); setSearching(false); setQuery('')
  }

  const applyFilter = (o: FilterOverrides) => {
    clearSearch(); setPublicList(null); setMineList(null)
    if (libraryFilter === 'public') void loadPublicList(0, o)
    else if (token) void loadMineList(0, o, token)
  }

  const onFilterTab = (filter: LibraryFilter) => {
    clearSearch()
    if (filter === libraryFilter) return
    setLibraryFilter(filter)
    if (filter === 'public') setPublicList(null)
    else setMineList(null)
  }

  const onSearchSubmit = (event: FormEvent) => {
    event.preventDefault()
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    void doSearch(query)
  }

  const listShown = libraryFilter === 'public' ? publicList : mineList
  const filterTabText: Record<LibraryFilter, string> = { public: '公开', workspace: '工作区共享', agent: 'Agent 私有' }

  return <section className="view-head">
    <p className="kicker"><i></i>Experience Library</p>
    <h1>经验库。</h1>
    <p className="sub">浏览和检索网络中的经验。公开经验无需登录；工作区共享与 Agent 私有记忆需要登录后可见。</p>

    <div className="filter-tabs" role="tablist" aria-label="经验范围">
      {(['public', 'workspace', 'agent'] as LibraryFilter[]).map(filter => (
        <button type="button" key={filter} role="tab" aria-selected={libraryFilter === filter} className={libraryFilter === filter ? 'on' : ''} onClick={() => onFilterTab(filter)}>{filterTabText[filter]}</button>
      ))}
    </div>

    {libraryFilter !== 'public' && !token && <div className="empty">
      <p>「{filterTabText[libraryFilter]}」需要登录后查看。</p>
      <button type="button" className="btn-primary" onClick={() => navigate('console')}>去控制台登录</button>
    </div>}

    {(libraryFilter === 'public' || token) && <>
      <form className="search-form" onSubmit={onSearchSubmit} role="search">
        <input value={query} onChange={event => onQueryChange(event.target.value)} placeholder="例如：Axum 连接 PostgreSQL 超时" aria-label="检索技术问题" />
        {query && <button type="button" className="search-clear" aria-label="清除检索词" onClick={clearSearch}>×</button>}
        <span className="search-hint">回车检索</span>
      </form>

      {!searchResults && <div className="filter-bar">
        <select value={filterOutcome} onChange={event => { setFilterOutcome(event.target.value); applyFilter({ outcome: event.target.value }) }} aria-label="结果类型">
          <option value="">全部结果</option>
          <option value="success">成功</option>
          <option value="failure">失败</option>
          <option value="partial">部分成功</option>
          <option value="unknown">结果未知</option>
        </select>
        <select value={filterTime} onChange={event => { setFilterTime(event.target.value); applyFilter({ time: event.target.value }) }} aria-label="时间范围">
          <option value="">全部时间</option>
          <option value="1d">最近 1 天</option>
          <option value="3d">最近 3 天</option>
          <option value="1w">最近 1 周</option>
          <option value="1m">最近 1 月</option>
        </select>
        <select value={filterSort} onChange={event => { setFilterSort(event.target.value); applyFilter({ sort: event.target.value }) }} aria-label="排序方式">
          <option value="latest">最新发布</option>
          <option value="reuse">复用最多</option>
          <option value="feedback">反馈最多</option>
          <option value="evidence">证据最多</option>
        </select>
      </div>}

      {searchResults
        ? <>
          <p className="lib-meta">{searching ? '检索中…' : `${searchResults.length} 条相关经验`} · <button type="button" className="text-btn" onClick={clearSearch}>返回浏览全部</button></p>
          {searchResults.length > 0
            ? <div className="cards lib-list">{searchResults.map(item => <MemoryCard item={item} onOpen={openMemory} key={item.id} />)}</div>
            : !searching && <div className="empty">没有找到相关经验。换个关键词试试，或让 Agent 写入第一条。</div>}
        </>
        : <>
          <p className="lib-meta">{listShown ? `共 ${listShown.total} 条${libraryFilter === 'public' ? '公开' : ''}经验` : '正在加载…'}</p>
          {listShown ? <>
            <div className="cards lib-list">{listShown.items.map(item => <MemoryCard item={item} onOpen={openMemory} key={item.id} />)}</div>
            {listShown.total === 0 && <div className="empty">暂无匹配的经验。</div>}
            {listShown.total > listShown.items.length && <button type="button" className="btn-ghost load-more" disabled={loading} onClick={() => libraryFilter === 'public' ? void loadPublicList(listShown.offset + listShown.limit) : void loadMineList(listShown.offset + listShown.limit)}>{loading ? '加载中' : '加载更多'}</button>}
          </> : <SkeletonCards count={3} />}
        </>}
    </>}
  </section>
}
