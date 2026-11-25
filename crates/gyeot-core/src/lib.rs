//! # gyeot-core
//!
//! 곁의 순수·결정적 코어(`docs/contracts.md`). 시계·파일·네트워크·스레드를 쓰지 않고, 지금 시각은
//! 늘 인자로 받는다. `wasm32-unknown-unknown`에서도 빌드된다.
//!
//! - [`normalize`]: 원본 훅·로그 페이로드 → [`Event`](§2).
//! - [`Engine`]: 이벤트 → 항목(§3), 존재감·전달(§4), 제안(§5), 세션(§7).
//! - [`Learn`]: `learn.json`에 저장하는 반응 집계와 조정.

pub mod engine;
pub mod event;
pub mod learn;
pub mod model;
pub mod normalize;
pub mod paths;
pub mod policy;
pub mod text;

pub use engine::{ActError, Action, ChannelInfo, DecisionInfo, Effect, Engine, VERSION};
pub use event::{Agent, Event, EventBody, Source};
pub use learn::{DISMISS_REASONS, Learn};
pub use model::{
    Budget, Delivery, Evidence, Item, ItemKind, ItemState, Level, Mood, Presence, Proposal,
    Session, SessionState, SettingsView, Snapshot, item_id,
};
pub use normalize::normalize;
pub use policy::{Policy, QuietRange};
