//! Declarative quest journal: yarn-variable state → quest-log entries.
//!
//! Yarn-only quests (fetch/talk) have no Python script, so nothing imperative
//! ever writes their progress into the Log panel. Instead each quest may ship
//! a YAML declaration — `assets/journal/<stem>.yaml` or
//! `assets/modules/<mod>/journal/<stem>.yaml` — whose id (file stem, module-
//! prefixed) matches the quest id convention. Each declaration maps the
//! player's yarn variables to a rendered `Quests` log entry:
//!
//! ```yaml
//! title: Down the Shaft
//! stages:                    # ordered; EVERY matching stage renders a note
//!   - when: hollow_bell_shaft_started          # bare string = truthy
//!     text: "Crawlers culled: {$hollow_bell_crawlers}/8."
//!   - when: { var: hollow_bell_shaft_done, is: true }   # exact match form
//!     text: The haulage-way is clear.
//!     completed: true        # appends " (complete)" to the entry title
//! ```
//!
//! The rendered body is the full history: one note per matching stage, in
//! declaration order, separated by [`BODY_DIVIDER`] lines (drawn as
//! horizontal rules by the log panel). The LAST matching stage decides the
//! `completed` title suffix. Quest flags are cumulative by convention (a
//! dialog sets `_started` and later adds `_done` without clearing the
//! former), so earlier notes stay visible as the quest advances.
//!
//! `evaluate_quest_journals` polls each player's variable-store generation
//! counter and re-renders on change, emitting `GameCommand::UpsertLogEntry`
//! so the entry flows through the normal log command path (caps, owner
//! gating, replication) — identical in all three runtime modes.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_yarnspinner::prelude::{VariableStorage, YarnValue};
use serde::Deserialize;

use crate::crafting::CharacterStash;
use crate::dialog::resources::CharacterVarStores;
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::log::{LogOwner, LogState, MAX_BODY_LEN, MAX_TITLE_LEN, QUESTS_SECTION};
use crate::player::components::{Player, PlayerIdentity};

/// Appended to the entry title when the winning stage has `completed: true`.
pub const COMPLETED_SUFFIX: &str = " (complete)";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalDecl {
    pub title: String,
    pub stages: Vec<JournalStage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalStage {
    pub when: StageCondition,
    pub text: String,
    #[serde(default)]
    pub completed: bool,
}

/// `when: some_var` (truthy) or `when: { var: some_var, is: value }` (exact).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StageCondition {
    Truthy(String),
    Equals { var: String, is: CondValue },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CondValue {
    Boolean(bool),
    Number(f64),
    String(String),
}

impl StageCondition {
    fn var(&self) -> &str {
        match self {
            Self::Truthy(var) => var,
            Self::Equals { var, .. } => var,
        }
    }

    fn matches(&self, vars: &HashMap<String, YarnValue>) -> bool {
        let Some(value) = vars.get(&ensure_dollar(self.var())) else {
            return false;
        };
        match self {
            Self::Truthy(_) => match value {
                YarnValue::Boolean(b) => *b,
                YarnValue::Number(n) => *n != 0.0,
                YarnValue::String(s) => !s.is_empty(),
            },
            Self::Equals { is, .. } => match (value, is) {
                (YarnValue::Boolean(a), CondValue::Boolean(b)) => a == b,
                (YarnValue::Number(a), CondValue::Number(b)) => f64::from(*a) == *b,
                (YarnValue::String(a), CondValue::String(b)) => a == b,
                _ => false,
            },
        }
    }
}

/// A journal entry rendered for one player from one declaration.
#[derive(Debug, PartialEq, Eq)]
pub struct RenderedEntry {
    pub title: String,
    pub body: String,
}

/// All loaded journal declarations, keyed by quest id (`<module>/<stem>` for
/// module files, bare `<stem>` for core ones).
#[derive(Resource, Default)]
pub struct QuestJournalRegistry {
    pub by_id: std::collections::BTreeMap<String, JournalDecl>,
}

impl QuestJournalRegistry {
    /// True when a declarative journal owns this quest id — the auto-journal
    /// path in `quest::systems` must then leave the entry alone.
    pub fn has_quest(&self, quest_id: &str) -> bool {
        self.by_id.contains_key(quest_id)
    }
}

/// Load every `journal/*.yaml` across asset roots and modules. A malformed
/// file is logged and skipped; it never takes the others down.
pub fn load_quest_journals() -> QuestJournalRegistry {
    let mut registry = QuestJournalRegistry::default();
    for asset in crate::assets::discover_yaml_assets("journal", "quest journal") {
        match parse_journal_decl(&asset.contents) {
            Ok(decl) => {
                registry.by_id.insert(asset.id, decl);
            }
            Err(err) => {
                error!(
                    "quest journal {} ({}) skipped: {err}",
                    asset.id,
                    asset.path.display()
                );
            }
        }
    }
    if !registry.by_id.is_empty() {
        info!("quest journal loaded {} declarations", registry.by_id.len());
    }
    registry
}

/// Parse + validate one declaration. Split from the loader so the repo-wide
/// content test can assert zero failures.
pub fn parse_journal_decl(contents: &str) -> Result<JournalDecl, String> {
    let decl: JournalDecl = serde_yaml::from_str(contents).map_err(|e| e.to_string())?;
    if decl.title.trim().is_empty() {
        return Err("title must not be empty".to_owned());
    }
    if decl.title.chars().count() + COMPLETED_SUFFIX.chars().count() > MAX_TITLE_LEN {
        return Err(format!("title exceeds {MAX_TITLE_LEN} chars"));
    }
    if decl.stages.is_empty() {
        return Err("stages must not be empty".to_owned());
    }
    let mut total_text = 0;
    for stage in &decl.stages {
        if stage.when.var().trim().is_empty() {
            return Err("stage `when` variable must not be empty".to_owned());
        }
        // +5 for a "\n---\n" divider; every stage can match at once.
        total_text += stage.text.chars().count() + 5;
    }
    // Static bound; interpolation can only shrink (`{$var}` → a number).
    if total_text > MAX_BODY_LEN {
        return Err(format!("combined stage text exceeds {MAX_BODY_LEN} chars"));
    }
    Ok(decl)
}

/// Render the entry for one player: every stage whose `when` matches becomes
/// a note, in declaration order, divider-separated. The LAST matching stage
/// decides the `completed` title suffix. No match → the quest hasn't started
/// for this player → no entry.
pub fn evaluate(decl: &JournalDecl, vars: &HashMap<String, YarnValue>) -> Option<RenderedEntry> {
    let matched: Vec<&JournalStage> = decl
        .stages
        .iter()
        .filter(|s| s.when.matches(vars))
        .collect();
    let last = matched.last()?;
    let mut title = decl.title.clone();
    if last.completed {
        title.push_str(COMPLETED_SUFFIX);
    }
    let body = matched
        .iter()
        .map(|s| interpolate(&s.text, vars).trim().to_owned())
        .filter(|note| !note.is_empty())
        .collect::<Vec<_>>()
        .join(&format!("\n{}\n", crate::log::BODY_DIVIDER));
    Some(RenderedEntry { title, body })
}

/// Replace `{$var}` tokens with the variable's rendered value. Whole-number
/// yarn Numbers render without the fractional part (`3`, not `3.0`). A
/// missing variable renders `?` and warns — loud but non-fatal.
fn interpolate(template: &str, vars: &HashMap<String, YarnValue>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{$") {
        out.push_str(&rest[..start]);
        let after_brace = &rest[start + 1..];
        let Some(end) = after_brace.find('}') else {
            // Unterminated token: emit verbatim and stop scanning.
            out.push_str(&rest[start..]);
            return out;
        };
        let var_name = &after_brace[..end];
        match vars.get(var_name) {
            Some(value) => out.push_str(&render_value(value)),
            None => {
                warn!("quest journal template references unset variable {var_name}");
                out.push('?');
            }
        }
        rest = &after_brace[end + 1..];
    }
    out.push_str(rest);
    out
}

fn render_value(value: &YarnValue) -> String {
    match value {
        YarnValue::Number(n) if n.fract() == 0.0 => format!("{}", *n as i64),
        YarnValue::Number(n) => n.to_string(),
        YarnValue::Boolean(b) => b.to_string(),
        YarnValue::String(s) => s.clone(),
    }
}

fn ensure_dollar(name: &str) -> String {
    if name.starts_with('$') {
        name.to_owned()
    } else {
        format!("${name}")
    }
}

/// Re-render every declaration for every player whose yarn variables changed
/// since the last pass, and upsert entries that differ from the player's
/// current log. Runs before `CommandIntercept` so `process_log_commands`
/// drains the upserts the same frame (commands still queued when
/// `process_game_commands` runs are dropped).
pub fn evaluate_quest_journals(
    registry: Res<QuestJournalRegistry>,
    var_stores: Res<CharacterVarStores>,
    mut seen: Local<std::collections::HashMap<u64, u64>>,
    players: Query<(&PlayerIdentity, &CharacterStash), With<Player>>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if registry.by_id.is_empty() {
        return;
    }
    for (identity, stash) in &players {
        let player_id = identity.id.0;
        let Some(store) = var_stores.by_player.get(&player_id) else {
            // No variables yet (never opened a dialog) → nothing can match.
            continue;
        };
        let generation = store.generation();
        if seen.get(&player_id) == Some(&generation) {
            continue;
        }
        seen.insert(player_id, generation);

        let vars = store.variables();
        let log = LogState::from_stash(stash);
        for (quest_id, decl) in &registry.by_id {
            let Some(rendered) = evaluate(decl, &vars) else {
                continue;
            };
            let current = log.entry(QUESTS_SECTION, quest_id);
            if current.is_some_and(|e| e.title == rendered.title && e.body == rendered.body) {
                continue;
            }
            pending_commands.push_for_player(
                identity.id,
                GameCommand::UpsertLogEntry {
                    section: QUESTS_SECTION.to_owned(),
                    subsection: quest_id.clone(),
                    title: rendered.title,
                    body: rendered.body,
                    owner: LogOwner::Engine,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(entries: &[(&str, YarnValue)]) -> HashMap<String, YarnValue> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn decl(yaml: &str) -> JournalDecl {
        parse_journal_decl(yaml).expect("test decl must parse")
    }

    const SHAFT: &str = r#"
title: Down the Shaft
stages:
  - when: shaft_started
    text: "Crawlers culled: {$crawlers}/8."
  - when: { var: shaft_ready, is: true }
    text: Report back to Marten.
  - when: shaft_done
    text: The haulage-way is clear.
    completed: true
"#;

    #[test]
    fn no_stage_matches_yields_no_entry() {
        assert_eq!(evaluate(&decl(SHAFT), &vars(&[])), None);
        // Declared-but-false is still "not started".
        let v = vars(&[("$shaft_started", YarnValue::Boolean(false))]);
        assert_eq!(evaluate(&decl(SHAFT), &v), None);
    }

    #[test]
    fn matching_stages_accumulate_divider_separated_notes() {
        let v = vars(&[
            ("$shaft_started", YarnValue::Boolean(true)),
            ("$shaft_ready", YarnValue::Boolean(true)),
            ("$crawlers", YarnValue::Number(8.0)),
        ]);
        let entry = evaluate(&decl(SHAFT), &v).unwrap();
        assert_eq!(entry.title, "Down the Shaft");
        assert_eq!(
            entry.body,
            "Crawlers culled: 8/8.\n---\nReport back to Marten."
        );
    }

    #[test]
    fn non_matching_stage_is_skipped_in_history() {
        // `shaft_ready` false → its note is absent even though later stages match.
        let v = vars(&[
            ("$shaft_started", YarnValue::Boolean(true)),
            ("$crawlers", YarnValue::Number(8.0)),
            ("$shaft_done", YarnValue::Boolean(true)),
        ]);
        let entry = evaluate(&decl(SHAFT), &v).unwrap();
        assert_eq!(
            entry.body,
            "Crawlers culled: 8/8.\n---\nThe haulage-way is clear."
        );
    }

    #[test]
    fn last_matching_stage_decides_completed_suffix() {
        let v = vars(&[("$shaft_done", YarnValue::Boolean(true))]);
        let entry = evaluate(&decl(SHAFT), &v).unwrap();
        assert_eq!(entry.title, "Down the Shaft (complete)");
        assert_eq!(entry.body, "The haulage-way is clear.");
    }

    #[test]
    fn interpolation_renders_whole_numbers_without_fraction() {
        let v = vars(&[
            ("$shaft_started", YarnValue::Boolean(true)),
            ("$crawlers", YarnValue::Number(3.0)),
        ]);
        let entry = evaluate(&decl(SHAFT), &v).unwrap();
        assert_eq!(entry.body, "Crawlers culled: 3/8.");
    }

    #[test]
    fn interpolation_missing_variable_renders_question_mark() {
        let v = vars(&[("$shaft_started", YarnValue::Boolean(true))]);
        let entry = evaluate(&decl(SHAFT), &v).unwrap();
        assert_eq!(entry.body, "Crawlers culled: ?/8.");
    }

    #[test]
    fn truthy_semantics_for_numbers_and_strings() {
        let d = decl("title: T\nstages:\n  - when: v\n    text: yes\n");
        for (value, expected) in [
            (YarnValue::Number(0.0), false),
            (YarnValue::Number(2.0), true),
            (YarnValue::String(String::new()), false),
            (YarnValue::String("x".to_owned()), true),
        ] {
            let v = vars(&[("$v", value)]);
            assert_eq!(evaluate(&d, &v).is_some(), expected);
        }
    }

    #[test]
    fn equals_condition_requires_matching_type_and_value() {
        let d = decl("title: T\nstages:\n  - when: { var: v, is: 3 }\n    text: hit\n");
        assert!(evaluate(&d, &vars(&[("$v", YarnValue::Number(3.0))])).is_some());
        assert!(evaluate(&d, &vars(&[("$v", YarnValue::Number(2.0))])).is_none());
        assert!(evaluate(&d, &vars(&[("$v", YarnValue::String("3".to_owned()))])).is_none());
    }

    #[test]
    fn dollar_prefix_in_when_is_accepted() {
        let d = decl("title: T\nstages:\n  - when: \"$v\"\n    text: hit\n");
        assert!(evaluate(&d, &vars(&[("$v", YarnValue::Boolean(true))])).is_some());
    }

    #[test]
    fn parse_rejects_bad_declarations() {
        assert!(parse_journal_decl("title: T\nstages: []\n").is_err());
        assert!(parse_journal_decl("stages:\n  - when: v\n    text: x\n").is_err());
        assert!(
            parse_journal_decl("title: T\nbogus: 1\nstages:\n  - when: v\n    text: x\n").is_err()
        );
    }

    /// Variables referenced in a template: the `{$name}` tokens.
    fn template_vars(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("{$") {
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else { break };
            out.push(after[..end].to_owned());
            rest = &after[end + 1..];
        }
        out
    }

    /// Every shipped journal file must parse, and every yarn variable it
    /// reads (stage conditions and `{$var}` template tokens) must be
    /// `<<declare>>`d in some shipped yarn file — otherwise the stage can
    /// never match (or renders `?`) and the bug is invisible until a player
    /// hits it.
    #[test]
    fn repo_journal_files_parse_and_reference_declared_vars() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        let mut journal_files = Vec::new();
        let push_yamls = |dir: &std::path::Path, journal_files: &mut Vec<std::path::PathBuf>| {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                    journal_files.push(path);
                }
            }
        };
        push_yamls(&manifest_dir.join("assets/journal"), &mut journal_files);
        if let Ok(modules) = std::fs::read_dir(manifest_dir.join("assets/modules")) {
            for module in modules.flatten() {
                push_yamls(&module.path().join("journal"), &mut journal_files);
            }
        }
        journal_files.sort();
        assert!(
            !journal_files.is_empty(),
            "no journal .yaml files found under assets/ — glob rot?"
        );

        // Collect every `<<declare $name` across shipped yarn.
        let mut yarn_files = Vec::new();
        for root in ["assets/dialogs", "assets/modules"] {
            crate::dialog::plugin::collect_yarn_files(&manifest_dir.join(root), &mut yarn_files);
        }
        let mut declared: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for path in &yarn_files {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            for (pos, _) in source.match_indices("<<declare $") {
                let name: String = source[pos + "<<declare $".len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                declared.insert(name);
            }
        }

        let mut failures = Vec::new();
        for path in &journal_files {
            let contents = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            let decl = match parse_journal_decl(&contents) {
                Ok(decl) => decl,
                Err(err) => {
                    failures.push(format!("{}: failed to parse: {err}", path.display()));
                    continue;
                }
            };
            for stage in &decl.stages {
                let mut referenced = vec![stage.when.var().trim_start_matches('$').to_owned()];
                referenced.extend(
                    template_vars(&stage.text)
                        .into_iter()
                        .map(|v| v.trim_start_matches('$').to_owned()),
                );
                for var in referenced {
                    if !declared.contains(&var) {
                        failures.push(format!(
                            "{}: references variable ${var} not <<declare>>d in any yarn file",
                            path.display()
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} journal problem(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
