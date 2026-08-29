export const PASSWORD_RULE = /^(?=.*[A-Za-z])(?=.*\d).{8,}$/

export const checkPassword = (password: string, confirm: string, onToast: (text: string, kind?: 'info' | 'error') => void) => {
  if (password !== confirm) { onToast('两次输入的密码不一致。', 'error'); return false }
  if (!PASSWORD_RULE.test(password)) { onToast('密码至少 8 位，且需同时包含字母和数字。', 'error'); return false }
  return true
}
