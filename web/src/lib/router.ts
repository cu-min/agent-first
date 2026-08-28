import { useEffect, useState } from 'react'

export type Page = 'overview' | 'library' | 'console'
export type Route = { page: Page; memoryId: string | null }

export const parseRoute = (): Route => {
  const parts = window.location.hash.replace(/^#\/?/, '').split('/').filter(Boolean)
  const page: Page = parts[0] === 'library' || parts[0] === 'console' ? parts[0] : 'overview'
  const memoryIndex = parts.indexOf('memory')
  const memoryId = memoryIndex >= 0 ? (parts[memoryIndex + 1] ?? null) : null
  return { page, memoryId }
}

export const navigate = (path: string) => {
  const next = `#/${path}`
  if (window.location.hash !== next) window.location.hash = next
}

export const useRoute = (): Route => {
  const [route, setRoute] = useState<Route>(parseRoute)
  useEffect(() => {
    const onChange = () => setRoute(parseRoute())
    window.addEventListener('hashchange', onChange)
    return () => window.removeEventListener('hashchange', onChange)
  }, [])
  return route
}
