import { describe, expect, it } from 'vitest'
import { navigate, parseRoute } from './router'

const setHash = (value: string) => {
  window.location.hash = value
}

describe('parseRoute', () => {
  it('空哈希回落到 overview', () => {
    setHash('')
    expect(parseRoute()).toEqual({ page: 'overview', memoryId: null })
  })

  it('仅斜杠也回落到 overview', () => {
    setHash('#/')
    expect(parseRoute()).toEqual({ page: 'overview', memoryId: null })
  })

  it('识别 library 与 console 页面', () => {
    setHash('#/library')
    expect(parseRoute().page).toBe('library')
    setHash('#/console')
    expect(parseRoute().page).toBe('console')
  })

  it('未知页面回落到 overview', () => {
    setHash('#/unknown-page')
    expect(parseRoute().page).toBe('overview')
  })

  it('提取 memory 段中的记忆 ID', () => {
    setHash('#/library/memory/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0')
    expect(parseRoute()).toEqual({ page: 'library', memoryId: '0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0' })
  })

  it('memory 段位于任意位置都能提取', () => {
    setHash('#/memory/abc123')
    expect(parseRoute()).toEqual({ page: 'overview', memoryId: 'abc123' })
  })

  it('memory 段缺 ID 时返回 null', () => {
    setHash('#/library/memory')
    expect(parseRoute().memoryId).toBeNull()
  })
})

describe('navigate', () => {
  it('写入带 #/ 前缀的哈希', () => {
    setHash('')
    navigate('library/memory/xyz')
    expect(window.location.hash).toBe('#/library/memory/xyz')
  })
})
