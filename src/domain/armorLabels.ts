type GameTextLocalizer = (value: string) => string;

const armorPartSuffix =
  /[·・‧](?:头部|身体|腕部|腰部|脚部|頭部|身體|腕部|腰部|腳部)$/u;

/**
 * 套装目标可能是五部位名称，也可能是头套、耳饰等单件外观。
 * 先提取共同套装名，再接受完整“服装”名称，最后才回退到现有中文名。
 */
export function armorTargetDisplayLabel(
  displayNames: readonly string[],
  modelId: string,
  localize: GameTextLocalizer,
): string {
  const localizedNames = displayNames
    .map((name) => localize(name).trim())
    .filter((name) => name && !name.toLocaleLowerCase().includes("dummy"));

  for (const name of localizedNames) {
    const setName = name.replace(armorPartSuffix, "");
    if (setName !== name && setName) {
      return setName;
    }
  }

  const standaloneOutfit = localizedNames.find((name) => /服装|服裝/u.test(name));
  return standaloneOutfit ?? localizedNames[0] ?? modelId;
}
