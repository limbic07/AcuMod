const naturalTextCollator = new Intl.Collator("zh-Hans-CN", {
  numeric: true,
  sensitivity: "base",
});

/** 把连续数字作为完整数值比较，例如 2 排在 10 前面。 */
export function compareNaturalText(left: string, right: string): number {
  return naturalTextCollator.compare(left, right);
}
