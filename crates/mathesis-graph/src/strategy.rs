//! 層4: 戦略・メタ層（Strategy/Meta Layer）のうち、証明戦略ノードの定義。
//!
//! 帰納法・背理法・対角線論法・随伴関手の利用など、証明を生成する手法そのものを
//! 高階ノード（Strategy Node）として扱う。数学の証明戦略は分野ごとに際限なく
//! 増えていくため、`JudgmentKind`/`MorphismKind` のような閉じた enum にはせず、
//! 「名前で引けるノード」として自由に追加できるようにする。よく使う名前は
//! `well_known` に定数として置くが、これは表記揺れを防ぐためのショートカットで
//! あって網羅的なカタログではない。DB 上は単なる `name`（一意）+ `description` の行。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StrategyId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyRecord {
    pub id: StrategyId,
    pub name: String,
    pub description: Option<String>,
}

/// よく使う戦略名の定数。`GraphStore::intern_strategy` にはこれ以外の任意の
/// 文字列も渡せる。
pub mod well_known {
    pub const INDUCTION: &str = "induction";
    pub const STRONG_INDUCTION: &str = "strong_induction";
    pub const CONTRADICTION: &str = "contradiction";
    pub const CONTRAPOSITIVE: &str = "contrapositive";
    pub const DIAGONALIZATION: &str = "diagonalization";
    pub const DIRECT_CONSTRUCTION: &str = "direct_construction";
    pub const CASE_SPLIT: &str = "case_split";
    pub const COMPACTNESS: &str = "compactness";
    pub const PIGEONHOLE: &str = "pigeonhole";
    pub const PROBABILISTIC_METHOD: &str = "probabilistic_method";
    pub const ADJOINT_FUNCTOR: &str = "adjoint_functor";
    pub const UNIVERSAL_PROPERTY: &str = "universal_property";
}
