import traditionalGameText from "../../references/mhwi-data/curated/game-text-zh-hant.json";

export type GameTextLanguage = "simplifiedChinese" | "traditionalChinese";

const traditionalNames = traditionalGameText.names as Record<string, string>;

/** 仅切换游戏内容名称；Acumod 自身界面始终使用简体中文。 */
export function localizeGameText(value: string, language: GameTextLanguage): string {
  return language === "traditionalChinese" ? (traditionalNames[value] ?? value) : value;
}

export function localizeGameTexts(values: string[], language: GameTextLanguage): string[] {
  return values.map((value) => localizeGameText(value, language));
}
