import armorMenuOrderData from "../../references/mhwi-data/curated/armor-menu-order.json";

interface ArmorReplacementLike {
  modelKind: string;
  modelId: string;
}

interface ArmorMenuOrderEntry {
  globalOrder: number;
}

const targetOrders = armorMenuOrderData.targetOrders as Record<string, ArmorMenuOrderEntry>;

/** 返回 MOD 命中的最早防具菜单位置；没有防具目标时排在已知防具之后。 */
export function earliestArmorMenuOrder(replacements: readonly ArmorReplacementLike[]): number {
  let earliestOrder = Number.MAX_SAFE_INTEGER;
  for (const replacement of replacements) {
    if (replacement.modelKind !== "armor") {
      continue;
    }
    const order = targetOrders[`armor:${replacement.modelId}`]?.globalOrder;
    if (order !== undefined) {
      earliestOrder = Math.min(earliestOrder, order);
    }
  }
  return earliestOrder;
}
