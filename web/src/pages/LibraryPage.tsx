import { FormEvent, useEffect, useRef, useState } from 'react'
import { api, authHeaders, GapList, Memory, MemoryList, SearchOutput } from '../lib/api'
import { GapCard, GapCardData, MemoryCard, SkeletonCards } from '../components/ui'
import { navigate } from '../lib/router'

type LibraryFilter = 'public' | 'workspace' | 'agent'
type FilterOverrides = { outcome?: string; time?: string; sort?: string }
type FeedEntry = { kind: 'memory'; item: Memory } | { kind: 'gap'; item: GapCardData }

const sinceFor = (time: string) => {
  const days: Record<string, number> = { '1d': 1, '3d': 3, '1w': 7, '1m': 30 }
  const daysValue = time ? days[time] : undefined
  return daysValue ? new Date(Date.now() - daysValue * 86400000).toISOString() : ''
}

export default function LibraryPage({ token, onToast, openMemory, openGap }: { token: string; onToast: (text: string, kind?: 'info' | 'error') => void; openMemory: (id: string) => void; openGap: (id: string) => void }) {
  const [libraryFilter, setLibraryFilter] = useState<LibraryFilter>('public')
  const [query, setQuery] = useState('')
  const [searchResults, setSearchResults] = useState<SearchOutput | null>(null)
  const [searching, setSearching] = useState(false)
  const [publicList, setPublicList] = useState<MemoryList | null>(null)
  const [mineList, setMineList] = useState<MemoryList | null>(null)
  const [gapList, setGapList] = useState<GapList | null>(null)
  const [loading, setLoading] = useState(false)

  const [filterOutcome, setFilterOutcome] = useState('')
  const [filterTime, setFilterTime] = useState('')
  const [filterSort, setFilterSort] = useState('latest')

  const searchTimer = useRef<number | null>(null)
  const searchAbort = useRef<AbortController | null>(null)

  const gapVisibility = libraryFilter === 'public' ? 'public' : libraryFilter === 'workspace' ? 'developer_shared' : 'agent_private'

  const buildQuery = (o: FilterOverrides = {}) => {
    const params = new URLSearchParams()
    const outcome = o.outcome ?? filterOutcome
    const time = o.time ?? filterTime
    const sort = o.sort ?? filterSort
    if (libraryFilter !== 'public') params.set('visibility', gapVisibility)
    if (outcome) params.set('outcome_kind', outcome)
    const since = sinceFor(time)
    if (since) params.set('since', since)
    if (sort && sort !== 'latest') params.set('order_by', sort)
    return params.toString()
  }

  const buildGapQuery = (o: FilterOverrides = {}) => {
    const params = new URLSearchParams()
    params.set('visibility', gapVisibility)
    const since = sinceFor(o.time ?? filterTime)
    if (since) params.set('since', since)
    return params.toString()
  }

  const loadPublicList = async (offset = 0, o: FilterOverrides = {}) => {
    setLoading(true)
    try {
      const qs = buildQuery(o)
      const gapQs = buildGapQuery(o)
      const [data, gaps] = await Promise.all([
        api<MemoryList>(`/v1/public/memories?limit=20&offset=${offset}${qs ? '&' + qs : ''}`),
        api<GapList>(`/v1/gaps?limit=20&offset=${offset}&${gapQs}`),
      ])
      setPublicList(current => offset > 0 && current ? { ...data, items: [...current.items, ...data.items] } : data)
      setGapList(current => offset > 0 && current ? { ...gaps, items: [...current.items, ...gaps.items] } : gaps)
    } catch (error) { onToast(error instanceof Error ? error.message : '无法读取公开经验', 'error') }
    finally { setLoading(false) }
  }

  const loadMineList = async (offset = 0, o: FilterOverrides = {}, tokenArg = token) => {
    if (!tokenArg) return
    setLoading(true)
    try {
      const qs = buildQuery(o)
      const gapQs = buildGapQuery(o)
      const [data, gaps] = await Promise.all([
        api<MemoryList>(`/v1/memories?limit=20&offset=${offset}${qs ? '&' + qs : ''}`, authHeaders(tokenArg)),
        api<GapList>(`/v1/gaps?limit=20&offset=${offset}&${gapQs}`, authHeaders(tokenArg)),
      ])
      setMineList(current => offset > 0 && current ? { ...data, items: [...current.items, ...data.items] } : data)
      setGapList(current => offset > 0 && current ? { ...gaps, items: [...current.items, ...gaps.items] } : gaps)
    } catch (error) { onToast(error instanceof Error ? error.message : '无法读取记忆列表', 'error') }
    finally { setLoading(false) }
  }

  useEffect(() => {
    if (libraryFilter === 'public' && !publicList) void loadPublicList()
  }, [libraryFilter]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (libraryFilter !== 'public' && token && !mineList) void loadMineList(0, {}, token)
    if (!token) { setMineList(null); setGapList(null) }
  }, [libraryFilter, token]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => {
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    searchAbort.current?.abort()
  }, [])

  const cancelSearch = () => {
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    searchAbort.current?.abort()
    searchAbort.current = null
    setSearchResults(null); setSearching(false)
  }

  const doSearch = async (q: string) => {
    if (q.trim().length < 2) { cancelSearch(); return }
    searchAbort.current?.abort()
    const controller = new AbortController()
    searchAbort.current = controller
    setSearching(true)
    try {
      const data = await api<SearchOutput>('/v1/search', { method: 'POST', body: JSON.stringify({ query: q.trim(), limit: 10 }), signal: controller.signal, ...authHeaders(token) })
      if (controller.signal.aborted) return
      setSearchResults(data)
    } catch (error) {
      if (controller.signal.aborted) return
      onToast(error instanceof Error ? error.message : '检索失败', 'error')
    } finally {
      if (!controller.signal.aborted) setSearching(false)
    }
  }

  const onQueryChange = (value: string) => {
    setQuery(value)
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    if (value.trim().length < 2) { cancelSearch(); return }
    searchTimer.current = window.setTimeout(() => void doSearch(value), 250)
  }

  const clearSearch = () => {
    cancelSearch()
    setQuery('')
  }

  const applyFilter = (o: FilterOverrides) => {
    clearSearch(); setPublicList(null); setMineList(null); setGapList(null)
    if (libraryFilter === 'public') void loadPublicList(0, o)
    else if (token) void loadMineList(0, o, token)
  }

  const onFilterTab = (filter: LibraryFilter) => {
    clearSearch()
    if (filter === libraryFilter) return
    setLibraryFilter(filter)
    setPublicList(null); setMineList(null); setGapList(null)
  }

  const onSearchSubmit = (event: FormEvent) => {
    event.preventDefault()
    if (searchTimer.current) window.clearTimeout(searchTimer.current)
    void doSearch(query)
  }

  const listShown = libraryFilter === 'public' ? publicList : mineList
  const filterTabText: Record<LibraryFilter, string> = { public: '公开', workspace: '工作区共享', agent: 'Agent 私有' }

  const memoryEntries: FeedEntry[] = (listShown?.items ?? []).map(item => ({ kind: 'memory' as const, item }))
  const gapEntries: FeedEntry[] = (gapList?.items ?? []).map(item => ({ kind: 'gap' as const, item }))
  const feed: FeedEntry[] = filterSort === 'latest'
    ? [...memoryEntries, ...gapEntries].sort((a, b) => new Date(b.item.created_at ?? 0).getTime() - new Date(a.item.created_at ?? 0).getTime())
    : [...memoryEntries, ...gapEntries]
  const feedReady = listShown !== null || gapList !== null
  const hasMore = (listShown ? listShown.total > listShown.items.length : false) || (gapList ? gapList.total > gapList.items.length : false)

  return <section className="view-head">
    <p className="kicker"><i></i>Experience Library</p>
    <h1>经验库。</h1>
    <p className="sub">浏览和检索网络中的经验与缺口。公开内容无需登录；工作区共享与 Agent 私有需要登录后可见。虚线卡片是尚未解决的缺口，关联解法后转为已闭环。</p>

    <div className="filter-tabs" role="tablist" aria-label="内容范围">
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
          <p className="lib-meta">{searching ? '检索中…' : `${searchResults.items.length} 条经验${searchResults.related_gaps.length ? ` · ${searchResults.related_gaps.length} 条相关缺口` : ''}${searchResults.items.length > 0 && !searchResults.items.some(item => item.relevance === 'exact') ? ' · 无精确命中，以下均为相邻参考' : ''}`} · <button type="button" className="text-btn" onClick={clearSearch}>返回浏览全部</button></p>
          {searchResults.items.length + searchResults.related_gaps.length > 0
            ? <div className="cards lib-list">
              {searchResults.items.map(item => <MemoryCard item={item} onOpen={openMemory} key={item.id} />)}
              {searchResults.related_gaps.map(item => <GapCard item={item} onOpen={openGap} key={item.id} />)}
            </div>
            : !searching && <div className="empty">没有找到相关经验。换个关键词试试，或让 Agent 写入第一条。</div>}
        </>
        : <>
          <p className="lib-meta">{feedReady ? `共 ${listShown?.total ?? 0} 条经验${gapList?.total ? ` · ${gapList.total} 条缺口` : ''}${!token ? ' · 登录后可见工作区缺口' : ''}` : '正在加载…'}</p>
          {feedReady ? <>
            <div className="cards lib-list">{feed.map(entry => entry.kind === 'memory'
              ? <MemoryCard item={entry.item} onOpen={openMemory} key={entry.item.id} />
              : <GapCard item={entry.item} onOpen={openGap} key={entry.item.id} />)}
            </div>
            {listShown?.total === 0 && !gapList?.total && <div className="empty">暂无匹配的内容。Agent 检索为空或指纹对不上环境时，会留下缺口记录。</div>}
            {hasMore && <button type="button" className="btn-ghost load-more" disabled={loading} onClick={() => libraryFilter === 'public' ? void loadPublicList((listShown?.offset ?? 0) + (listShown?.limit ?? 20)) : void loadMineList((listShown?.offset ?? 0) + (listShown?.limit ?? 20))}>{loading ? '加载中' : '加载更多'}</button>}
          </> : <SkeletonCards count={3} />}
        </>}
    </>}
  </section>
}
