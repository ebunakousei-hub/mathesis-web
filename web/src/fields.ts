export interface Label {
  ja: string;
  en: string;
}

export interface FieldNode {
  id: string;
  label: Label;
  children?: FieldNode[];
}

export interface BridgeNode {
  id: string;
  label: Label;
  /** 橋渡しする主要分野2つの id */
  between: [string, string];
}

/** 主要分野・下位分野・学際領域のどれかから id で表示ラベルを引く */
export function findLabelById(id: string): Label | undefined {
  for (const f of MAIN_FIELDS) {
    if (f.id === id) return f.label;
    const child = f.children?.find((c) => c.id === id);
    if (child) return child.label;
  }
  return BRIDGE_FIELDS.find((b) => b.id === id)?.label;
}

// 主要5分野。円周上に均等配置する。
export const MAIN_FIELDS: FieldNode[] = [
  {
    id: "analysis",
    label: { ja: "解析", en: "Analysis" },
    children: [
      { id: "real-analysis", label: { ja: "実解析", en: "Real analysis" } },
      { id: "complex-analysis", label: { ja: "複素解析", en: "Complex analysis" } },
      { id: "functional-analysis", label: { ja: "関数解析", en: "Functional analysis" } },
      { id: "harmonic-analysis", label: { ja: "調和解析", en: "Harmonic analysis" } },
    ],
  },
  {
    id: "number-theory",
    label: { ja: "数論", en: "Number theory" },
    children: [
      { id: "algebraic-number-theory", label: { ja: "代数的数論", en: "Algebraic number theory" } },
      { id: "analytic-number-theory", label: { ja: "解析的数論", en: "Analytic number theory" } },
      { id: "automorphic-forms", label: { ja: "保型形式論", en: "Automorphic forms" } },
    ],
  },
  {
    id: "geometry",
    label: { ja: "幾何", en: "Geometry" },
    children: [
      { id: "differential-geometry", label: { ja: "微分幾何", en: "Differential geometry" } },
      { id: "topology", label: { ja: "位相幾何", en: "Topology" } },
      { id: "riemannian-geometry", label: { ja: "リーマン幾何", en: "Riemannian geometry" } },
      { id: "algebraic-topology", label: { ja: "代数的位相幾何", en: "Algebraic topology" } },
    ],
  },
  {
    id: "algebra",
    label: { ja: "代数", en: "Algebra" },
    children: [
      { id: "group-theory", label: { ja: "群論", en: "Group theory" } },
      { id: "commutative-algebra", label: { ja: "可換環論", en: "Commutative algebra" } },
      { id: "lie-groups", label: { ja: "リー群", en: "Lie groups" } },
      { id: "representation-theory", label: { ja: "表現論", en: "Representation theory" } },
    ],
  },
  {
    id: "foundations",
    label: { ja: "基礎論", en: "Foundations" },
    children: [
      { id: "set-theory", label: { ja: "集合論", en: "Set theory" } },
      { id: "model-theory", label: { ja: "モデル理論", en: "Model theory" } },
      { id: "proof-theory", label: { ja: "証明論", en: "Proof theory" } },
      { id: "category-theory", label: { ja: "圏論", en: "Category theory" } },
    ],
  },
];

// 隣接する主要分野の「間」に位置する学際領域。
export const BRIDGE_FIELDS: BridgeNode[] = [
  {
    id: "analytic-number-theory-bridge",
    label: { ja: "解析的整数論", en: "Analytic number theory" },
    between: ["analysis", "number-theory"],
  },
  {
    id: "arithmetic-geometry",
    label: { ja: "数論幾何", en: "Arithmetic geometry" },
    between: ["number-theory", "geometry"],
  },
  {
    id: "algebraic-geometry",
    label: { ja: "代数幾何", en: "Algebraic geometry" },
    between: ["geometry", "algebra"],
  },
  {
    id: "categorical-algebra",
    label: { ja: "圏論的代数", en: "Categorical algebra" },
    between: ["algebra", "foundations"],
  },
  {
    id: "computability",
    label: { ja: "計算可能性理論", en: "Computability theory" },
    between: ["foundations", "analysis"],
  },
];
