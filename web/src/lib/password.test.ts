import { describe, expect, it, vi } from 'vitest'
import { PASSWORD_RULE, checkPassword } from './password'

const failures = () => {
  const onToast = vi.fn()
  return { onToast, ok: (password: string, confirm: string) => checkPassword(password, confirm, onToast) }
}

describe('checkPassword', () => {
  it('合法密码通过且不弹提示', () => {
    const { onToast, ok } = failures()
    expect(ok('password123', 'password123')).toBe(true)
    expect(onToast).not.toHaveBeenCalled()
  })

  it('两次输入不一致时拒绝', () => {
    const { onToast, ok } = failures()
    expect(ok('password123', 'password124')).toBe(false)
    expect(onToast).toHaveBeenCalledWith('两次输入的密码不一致。', 'error')
  })

  it('缺少数字的密码被拒绝', () => {
    const { onToast, ok } = failures()
    expect(ok('password', 'password')).toBe(false)
    expect(onToast).toHaveBeenCalledWith('密码至少 8 位，且需同时包含字母和数字。', 'error')
  })

  it('缺少字母的密码被拒绝', () => {
    const { onToast, ok } = failures()
    expect(ok('12345678', '12345678')).toBe(false)
    expect(onToast).toHaveBeenCalledWith('密码至少 8 位，且需同时包含字母和数字。', 'error')
  })

  it('过短的密码被拒绝', () => {
    const { ok } = failures()
    expect(ok('pw12345', 'pw12345')).toBe(false)
  })

  it('PASSWORD_RULE 与后端 8-256 位字母加数字规则一致', () => {
    expect(PASSWORD_RULE.test('a1b2c3d4')).toBe(true)
    expect(PASSWORD_RULE.test('a'.repeat(8))).toBe(false)
    expect(PASSWORD_RULE.test('1'.repeat(8))).toBe(false)
  })
})
