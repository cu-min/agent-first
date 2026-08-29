import { describe, expect, it } from 'vitest'
import { condText, fmtNum, relTime } from './api'

describe('relTime', () => {
  it('刚刚', () => {
    expect(relTime(new Date().toISOString())).toBe('刚刚')
  })

  it('分钟与小时级别', () => {
    const minutesAgo = (n: number) => new Date(Date.now() - n * 60_000).toISOString()
    expect(relTime(minutesAgo(1))).toBe('1 分钟前')
    expect(relTime(minutesAgo(59))).toBe('59 分钟前')
    expect(relTime(minutesAgo(90))).toBe('1 小时前')
  })

  it('天级别', () => {
    const daysAgo = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString()
    expect(relTime(daysAgo(3))).toBe('3 天前')
  })
})

describe('fmtNum', () => {
  it('千分位格式化', () => {
    expect(fmtNum(1234)).toBe('1,234')
    expect(fmtNum(1234567)).toBe('1,234,567')
    expect(fmtNum(0)).toBe('0')
  })
})

describe('condText', () => {
  it('字符串原样返回', () => {
    expect(condText('Windows 11')).toBe('Windows 11')
  })

  it('对象序列化为缩进 JSON', () => {
    expect(condText({ technologies: ['postgres 17'] })).toBe('{\n  "technologies": [\n    "postgres 17"\n  ]\n}')
  })
})
