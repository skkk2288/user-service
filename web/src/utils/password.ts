/**
 * 密码强度校验（与后端 src/utils/password.rs 共享同一套规则）
 *
 * 规则：
 * 1. 长度 >= 8 且 <= 64
 * 2. 字符类别计数（满足 3 类即可）：
 *    - 大写字母 [A-Z]
 *    - 小写字母 [a-z]
 *    - 数字 [0-9]
 *    - 特殊字符 [^A-Za-z0-9]
 *
 * 校验通过条件：规则 1 满足 AND 规则 2 中至少 3 类满足
 */

export interface PasswordCheckResult {
  valid: boolean;          // 是否通过全部校验
  length: boolean;         // 长度 8-64
  hasUpper: boolean;       // 有大写
  hasLower: boolean;       // 有小写
  hasDigit: boolean;       // 有数字
  hasSpecial: boolean;     // 有特殊字符
  categoriesMet: number;   // 满足的类别数（0-4）
  strength: 'weak' | 'medium' | 'strong';  // 强度等级
}

const MIN_LENGTH = 8;
const MAX_LENGTH = 64;

/**
 * 计算密码中满足的字符类别数
 */
function countCategories(hasUpper: boolean, hasLower: boolean, hasDigit: boolean, hasSpecial: boolean): number {
  return [hasUpper, hasLower, hasDigit, hasSpecial].filter(Boolean).length;
}

/**
 * 计算强度等级（前端展示用）
 *
 * - weak: 长度 < 8，或 categoriesMet < 2
 * - medium: 长度 >= 8 且 categoriesMet == 2 或 3
 * - strong: 长度 >= 12 且 categoriesMet == 4
 */
function calcStrength(length: number, categoriesMet: number): 'weak' | 'medium' | 'strong' {
  if (length < MIN_LENGTH || categoriesMet < 2) {
    return 'weak';
  }
  if (length >= 12 && categoriesMet === 4) {
    return 'strong';
  }
  return 'medium';
}

/**
 * 校验密码强度
 *
 * @param password 待校验的密码明文
 * @returns PasswordCheckResult 结构化校验结果
 */
export function checkPasswordStrength(password: string): PasswordCheckResult {
  const length = password.length >= MIN_LENGTH && password.length <= MAX_LENGTH;
  const hasUpper = /[A-Z]/.test(password);
  const hasLower = /[a-z]/.test(password);
  const hasDigit = /[0-9]/.test(password);
  const hasSpecial = /[^A-Za-z0-9]/.test(password);

  const categoriesMet = countCategories(hasUpper, hasLower, hasDigit, hasSpecial);
  const valid = length && categoriesMet >= 3;

  return {
    valid,
    length,
    hasUpper,
    hasLower,
    hasDigit,
    hasSpecial,
    categoriesMet,
    strength: calcStrength(password.length, categoriesMet),
  };
}
