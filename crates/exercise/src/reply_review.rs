//! EXCON review of player replies: only the EXCON acts on the world.
//!
//! Players answer injects in free text and never propose actions. The AI reads
//! each reply thread (the inject, the reply, the questions sent and their
//! answers) and suggests actions drawn only from the command catalogue
//! (`sim::Command`). Each suggestion keeps its source (role and quoted words),
//! its parameters and the AI's notes. The EXCON executes, edits or ignores it;
//! only an executed suggestion becomes a command applied by
//! `sim::World::apply`.
//!
//! The clarification loop:
//! * a suggestion that lacks required parameters is *incomplete*
//!   (`sim::missing_params`): it is kept, never dropped and never guessed;
//! * complete suggestions can be executed at once, without waiting;
//! * the AI drafts one question per player bundling every gap (a template
//!   question is drafted when the model gave none). Drafts are never sent by
//!   themselves: [`Exercise::send_review_question`] is an explicit EXCON act;
//! * the player's answer is linked to its question; the AI re-reads the whole
//!   thread and [`Exercise::ingest_reading`] merges the new reading, telling
//!   the EXCON what was completed, changed or added. Decisions already taken
//!   (executed, ignored) and EXCON edits on unchanged items are preserved;
//! * a question may go to another role than the author (cross-role
//!   inconsistency), still only as a draft;
//! * the EXCON closes the review when it wants; open questions stay listed.
//!
//! Model code: no HTTP, no LLM. The caller runs the model outside the lock
//! and hands its JSON answer to [`Exercise::ingest_reading`], which validates it.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sim::{Actor, Command, Verdict};

use crate::queue::ExconWhen;
use crate::{ExecutionRecord, Exercise, MsgKind, Phase, ProposalStatus, Source, MASTER};

/// Suggestions kept per reply (the model may return fewer).
const MAX_ITEMS: usize = 12;
/// Longest question text the EXCON can send.
const MAX_QUESTION: usize = 2000;

/// Where a suggestion stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    /// Every required parameter is known: the EXCON may execute it.
    Complete,
    /// Some required parameters are missing (see [`ReviewItem::missing`]).
    Incomplete,
    /// The EXCON decided to run it: it waits in the EXCON queue
    /// ([`crate::queue`]) for the next advance (see [`ReviewItem::queued`]).
    Queued,
    /// The EXCON executed it (see [`ReviewItem::execution`]).
    Executed,
    /// The EXCON executed it but the simulator refused it.
    Failed,
    /// The EXCON decided not to act on it.
    Ignored,
}

/// What the latest reading did to a suggestion (the diff shown to the EXCON).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    New,
    /// Was incomplete, now has every required parameter.
    Completed,
    /// Parameters or role differ from the previous reading.
    Changed,
    Unchanged,
}

/// One action the AI suggests from a reply thread.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem {
    pub id: u64,
    /// The player reply whose thread the suggestion comes from.
    pub proposal: u64,
    /// Role that would carry the action out (its resources and authority).
    pub role: String,
    /// Role whose words the suggestion comes from.
    pub source_role: String,
    /// The player's words it comes from (verbatim).
    pub quote: String,
    /// Command type (`"CutFireLine"`, ...).
    pub kind: String,
    /// Parameters known so far (the command's fields without `"type"`).
    pub params: Value,
    /// Required parameters still missing; empty when complete.
    pub missing: Vec<String>,
    /// AI notes: ETA, feasibility, inconsistencies.
    pub notes: Vec<String>,
    /// Feasibility of a complete suggestion now.
    pub verdict: Option<Verdict>,
    /// Why a complete-looking suggestion is not a valid command (bad value).
    pub invalid: Option<String>,
    pub description: String,
    pub state: ItemState,
    pub change: Change,
    /// Parameters before the latest change (diff), when changed.
    pub previous: Option<Value>,
    /// The EXCON edited the parameters: later readings fill only what is missing.
    pub edited: bool,
    /// Reading that last touched it.
    pub revision: u32,
    pub execution: Option<ExecutionRecord>,
    /// Id of its queued EXCON action while it waits for play.
    pub queued: Option<u64>,
    pub note: Option<String>,
    pub turn: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    /// Drafted (by the AI or the EXCON), waiting for EXCON review. Never sent by itself.
    Draft,
    /// Sent by the EXCON; waiting for the answer.
    Sent,
    Answered,
    Discarded,
}

/// A question to one player about a reply thread.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewQuestion {
    pub id: u64,
    pub proposal: u64,
    pub to: String,
    /// In the recipient's language: what the EXCON reviews, edits and sends verbatim.
    pub text: String,
    /// The recipient's language.
    pub language: String,
    /// English working copy, made when the question is sent.
    pub text_en: Option<String>,
    /// Suggestions it asks about.
    pub about: Vec<u64>,
    /// Addressed to another role than the reply's author.
    pub cross_role: bool,
    /// Why the AI asks (shown to the EXCON only).
    pub reason: String,
    /// Drafted from a template because the model gave no question for a gap.
    pub template: bool,
    pub persona: Option<String>,
    pub state: QuestionState,
    pub message: Option<u64>,
    pub answer: Option<u64>,
    pub revision: u32,
    pub turn: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    /// The AI is reading the thread.
    Reading,
    Ready,
    /// The AI could not read it; the EXCON acts by hand (Controllo).
    Failed,
    /// The EXCON closed the review.
    Closed,
}

/// The diff of the latest reading.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReadingDiff {
    pub completed: Vec<u64>,
    pub changed: Vec<u64>,
    pub new: Vec<u64>,
    /// Model actions left out: not catalogue actions, or not the players' words.
    pub dropped: usize,
}

/// The review of one player reply.
#[derive(Debug, Clone, Serialize)]
pub struct ReplyReview {
    pub proposal: u64,
    pub role: String,
    pub inject_id: Option<u64>,
    pub revision: u32,
    pub status: ReviewStatus,
    pub summary: String,
    pub error: Option<String>,
    pub diff: ReadingDiff,
    pub turn: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Board {
    pub reviews: Vec<ReplyReview>,
    pub items: Vec<ReviewItem>,
    pub questions: Vec<ReviewQuestion>,
}

/// One action of a reading, as the model returns it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadAction {
    /// Existing suggestion this one updates (its id), if any.
    #[serde(default, rename = "ref")]
    pub ref_id: Option<u64>,
    /// Role that would execute it (default: the reply's author).
    #[serde(default)]
    pub role: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub quote: String,
    /// Role whose words the quote is (default: the reply's author).
    #[serde(default)]
    pub source_role: String,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// One question of a reading.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadQuestion {
    pub to: String,
    pub text: String,
    /// Indexes into [`Reading::actions`] the question is about.
    #[serde(default)]
    pub about: Vec<usize>,
    #[serde(default)]
    pub reason: String,
}

/// What the model made of a reply thread (validated by [`Exercise::ingest_reading`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reading {
    #[serde(default)]
    pub actions: Vec<ReadAction>,
    #[serde(default)]
    pub questions: Vec<ReadQuestion>,
    #[serde(default)]
    pub summary: String,
    /// Actions the caller already left out (fidelity checks).
    #[serde(default, skip_serializing)]
    pub dropped: usize,
}

/// What the model may read for one reading.
#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub proposal: u64,
    pub role: String,
    pub revision: u32,
    /// The inject, the reply, the questions and answers, oldest first.
    pub thread: Value,
    /// Suggestions already made, with their ids and states.
    pub items: Value,
    /// Roles with their allowed actions and resources.
    pub roles: Value,
    /// Ground truth for ETA and feasibility notes.
    pub digest: Value,
    /// Every player text of the thread: quotes must come from it.
    pub player_words: Vec<String>,
    /// Who the EXCON speaks as in questions.
    pub persona: String,
}

impl Exercise {
    pub fn review(&self, pid: u64) -> Option<&ReplyReview> {
        self.review.reviews.iter().find(|r| r.proposal == pid)
    }

    fn review_mut(&mut self, pid: u64) -> Result<&mut ReplyReview> {
        self.review
            .reviews
            .iter_mut()
            .find(|r| r.proposal == pid)
            .ok_or_else(|| anyhow::anyhow!("reply {pid} has no review"))
    }

    fn item_mut(&mut self, id: u64) -> Result<&mut ReviewItem> {
        self.review
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown suggestion {id}"))
    }

    fn question_mut(&mut self, id: u64) -> Result<&mut ReviewQuestion> {
        self.review
            .questions
            .iter_mut()
            .find(|q| q.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown question {id}"))
    }

    /// Whether reply `pid` is reviewed this way: a player's narrative reply
    /// without simulator actions (players never propose actions).
    pub fn reviewable(&self, pid: u64) -> bool {
        self.proposal(pid).is_some_and(|p| {
            matches!(p.source, Source::Player(_))
                && p.commands.is_empty()
                && !p.original.trim().is_empty()
                && p.status != ProposalStatus::RolledBack
        })
    }

    /// Open the review of reply `pid` (or reopen it after an answer) and mark it
    /// as being read. Returns the reading revision the answer must carry.
    pub fn begin_review(&mut self, pid: u64) -> Result<u32> {
        if !self.reviewable(pid) {
            bail!("reply {pid} cannot be reviewed");
        }
        let p = self.proposal(pid).expect("reviewable");
        let (role, inject_id) = (p.source.label().to_string(), p.inject_id);
        let turn = self.turn;
        let revision = match self.review.reviews.iter_mut().find(|r| r.proposal == pid) {
            Some(r) => {
                r.revision += 1;
                r.status = ReviewStatus::Reading;
                r.error = None;
                r.revision
            }
            None => {
                self.review.reviews.push(ReplyReview {
                    proposal: pid,
                    role,
                    inject_id,
                    revision: 1,
                    status: ReviewStatus::Reading,
                    summary: String::new(),
                    error: None,
                    diff: ReadingDiff::default(),
                    turn,
                });
                1
            }
        };
        self.record(
            "review_reading",
            MASTER,
            json!({"proposal": pid, "revision": revision}),
        );
        Ok(revision)
    }

    /// Everything the model reads for the review of `pid`.
    pub fn review_context(&self, pid: u64) -> Result<ReviewContext> {
        let r = self
            .review(pid)
            .ok_or_else(|| anyhow::anyhow!("reply {pid} has no review"))?;
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown reply {pid}"))?;
        let mut thread = Vec::new();
        let mut words = Vec::new();
        if let Some(root) = p.inject_id {
            if let Some(m) = self
                .messages
                .iter()
                .find(|m| m.inject_id == Some(root) && m.kind == MsgKind::Inject)
            {
                thread.push(json!({"kind": "inject", "message": m.id, "from": m.persona.clone().unwrap_or_else(|| m.from.clone()), "clock": m.clock, "title": m.title, "text": m.text_en}));
            }
        }
        let reply_en = if p.translation_en.trim().is_empty() {
            p.original.clone()
        } else {
            p.translation_en.clone()
        };
        // What the player said the reply rests on (plan, procedure, law or
        // operational judgement, with their reference or rationale): context
        // for the reading, never an action by itself.
        let basis = p.evidence.as_ref().map(|e| json!({"kind": e.basis.kind, "reference": e.basis.reference}));
        thread.push(json!({"kind": "reply", "from": r.role, "clock": scenario::format_clock(self.world.scn.start_clock_s + p.t_s), "text": reply_en, "original": p.original, "basis": basis}));
        words.push(p.original.clone());
        words.push(reply_en);
        for q in self.review.questions.iter().filter(|q| q.proposal == pid) {
            if !matches!(q.state, QuestionState::Sent | QuestionState::Answered) {
                continue;
            }
            let asked = q
                .message
                .and_then(|id| self.messages.iter().find(|m| m.id == id));
            thread.push(json!({"kind": "question", "question": q.id, "to": q.to, "clock": asked.map(|m| m.clock.clone()), "text": q.text_en.clone().unwrap_or_else(|| q.text.clone()), "about": q.about}));
            if let Some(a) = q
                .answer
                .and_then(|id| self.messages.iter().find(|m| m.id == id))
            {
                thread.push(json!({"kind": "answer", "question": q.id, "from": a.from, "clock": a.clock, "text": a.text_en, "original": a.original}));
                words.push(a.original.clone().unwrap_or_else(|| a.text.clone()));
                words.push(a.text_en.clone());
            }
        }
        let items = self
            .review
            .items
            .iter()
            .filter(|i| i.proposal == pid)
            .map(|i| json!({"id": i.id, "role": i.role, "type": i.kind, "params": i.params, "missing": i.missing, "state": i.state, "quote": i.quote}))
            .collect::<Vec<_>>();
        let roles = self
            .world
            .scn
            .roles
            .iter()
            .map(|role| json!({"id": role.id, "name": role.name, "language": self.language_of(&role.id), "allowed_actions": role.allowed_actions, "resources": self.world.d.pools.get(&role.id)}))
            .collect::<Vec<_>>();
        // Other replies to the same inject, for cross-role inconsistencies.
        let others = self
            .proposals
            .iter()
            .filter(|o| o.id != pid && o.inject_id.is_some() && o.inject_id == p.inject_id && o.status != ProposalStatus::RolledBack)
            .map(|o| json!({"from": o.source.label(), "text": if o.translation_en.is_empty() { &o.original } else { &o.translation_en }}))
            .collect::<Vec<_>>();
        let digest = json!({
            "clock": self.world.clock(),
            "turn": self.turn,
            "world": self.world.digest(None),
            "deployments": self.world.deployment_view(),
            "resource_accounting": self.resource_accounting(None),
            "other_replies_to_this_inject": others,
        });
        let language = self.language_of(MASTER);
        let persona = self.persona_name(self.persona_for(None, p.inject_id).as_deref(), &language);
        Ok(ReviewContext {
            proposal: pid,
            role: r.role.clone(),
            revision: r.revision,
            thread: Value::Array(thread),
            items: Value::Array(items),
            roles: Value::Array(roles),
            digest,
            player_words: words,
            persona,
        })
    }

    /// The model could not read the thread.
    pub fn review_failed(&mut self, pid: u64, revision: u32, error: &str) -> Result<()> {
        let r = self.review_mut(pid)?;
        if r.revision != revision {
            bail!("an older reading of reply {pid} was discarded");
        }
        r.status = ReviewStatus::Failed;
        r.error = Some(error.chars().take(300).collect());
        self.record(
            "review_failed",
            MASTER,
            json!({"proposal": pid, "revision": revision, "error": error}),
        );
        Ok(())
    }

    /// Merge the model's reading of reply `pid` into its suggestions and
    /// question drafts. Nothing is executed and nothing is sent.
    pub fn ingest_reading(
        &mut self,
        pid: u64,
        revision: u32,
        reading: Reading,
    ) -> Result<ReadingDiff> {
        let r = self
            .review(pid)
            .ok_or_else(|| anyhow::anyhow!("reply {pid} has no review"))?;
        if r.revision != revision {
            bail!("an older reading of reply {pid} was discarded");
        }
        if r.status == ReviewStatus::Closed {
            bail!("the review of reply {pid} is closed");
        }
        let author = r.role.clone();
        let first = revision == 1;
        let mut diff = ReadingDiff {
            dropped: reading.dropped,
            ..Default::default()
        };
        // Index of the reading's action -> suggestion id.
        let mut placed: Vec<Option<u64>> = Vec::new();
        let mut touched: Vec<u64> = Vec::new();
        for a in reading.actions.iter() {
            if placed.iter().flatten().count() >= MAX_ITEMS
                || !sim::ALL_KINDS.contains(&a.kind.as_str())
                || sim::MASTER_ONLY.contains(&a.kind.as_str())
            {
                diff.dropped += 1;
                placed.push(None);
                continue;
            }
            let role = if self.players.contains_key(&a.role) {
                a.role.clone()
            } else {
                author.clone()
            };
            let source_role = if self.players.contains_key(&a.source_role) {
                a.source_role.clone()
            } else {
                author.clone()
            };
            let mut params = match &a.params {
                Value::Object(o) => Value::Object(o.clone()),
                _ => json!({}),
            };
            if let Some(o) = params.as_object_mut() {
                o.remove("type");
                o.retain(|_, v| !v.is_null());
            }
            let notes: Vec<String> = a
                .notes
                .iter()
                .map(|n| n.trim().chars().take(300).collect::<String>())
                .filter(|n| !n.is_empty())
                .take(4)
                .collect();
            let quote: String = a.quote.trim().chars().take(400).collect();
            let existing = self.match_item(pid, a, &touched);
            let id = match existing {
                Some(id) => {
                    self.update_item(id, role, params, notes, quote, revision, first, &mut diff)?;
                    id
                }
                None => {
                    let id = self.id();
                    self.review.items.push(ReviewItem {
                        id,
                        proposal: pid,
                        role,
                        source_role,
                        quote,
                        kind: a.kind.clone(),
                        params,
                        missing: Vec::new(),
                        notes,
                        verdict: None,
                        invalid: None,
                        description: String::new(),
                        state: ItemState::Incomplete,
                        change: if first {
                            Change::Unchanged
                        } else {
                            Change::New
                        },
                        previous: None,
                        edited: false,
                        revision,
                        execution: None,
                        queued: None,
                        note: None,
                        turn: self.turn,
                    });
                    if !first {
                        diff.new.push(id);
                    }
                    id
                }
            };
            touched.push(id);
            placed.push(Some(id));
        }
        // Items the reading did not mention keep their state and lose their diff mark.
        for i in self
            .review
            .items
            .iter_mut()
            .filter(|i| i.proposal == pid && !touched.contains(&i.id))
        {
            i.change = Change::Unchanged;
            i.previous = None;
        }
        self.recheck_items(pid);
        self.merge_questions(pid, revision, &reading, &placed, &author)?;
        let r = self.review_mut(pid)?;
        r.status = ReviewStatus::Ready;
        r.summary = reading.summary.trim().chars().take(500).collect();
        r.diff = diff.clone();
        self.record(
            "review_ready",
            MASTER,
            json!({"proposal": pid, "revision": revision, "diff": diff,
                "items": self.review.items.iter().filter(|i| i.proposal == pid).collect::<Vec<_>>(),
                "questions": self.review.questions.iter().filter(|q| q.proposal == pid).collect::<Vec<_>>()}),
        );
        Ok(diff)
    }

    /// The suggestion of `pid` a reading's action updates: its `ref`, else the
    /// same type and quote, else the only open one of that type.
    fn match_item(&self, pid: u64, a: &ReadAction, taken: &[u64]) -> Option<u64> {
        let mine = |i: &&ReviewItem| i.proposal == pid && !taken.contains(&i.id);
        if let Some(id) = a.ref_id {
            if let Some(i) = self
                .review
                .items
                .iter()
                .filter(mine)
                .find(|i| i.id == id && i.kind == a.kind)
            {
                return Some(i.id);
            }
        }
        let quote = a.quote.trim().to_lowercase();
        if let Some(i) = self
            .review
            .items
            .iter()
            .filter(mine)
            .find(|i| i.kind == a.kind && !quote.is_empty() && i.quote.to_lowercase() == quote)
        {
            return Some(i.id);
        }
        let open: Vec<&ReviewItem> = self
            .review
            .items
            .iter()
            .filter(mine)
            .filter(|i| i.kind == a.kind && i.state == ItemState::Incomplete)
            .collect();
        (open.len() == 1).then(|| open[0].id)
    }

    #[allow(clippy::too_many_arguments)]
    fn update_item(
        &mut self,
        id: u64,
        role: String,
        params: Value,
        notes: Vec<String>,
        quote: String,
        revision: u32,
        first: bool,
        diff: &mut ReadingDiff,
    ) -> Result<()> {
        let item = self.item_mut(id)?;
        if !notes.is_empty() {
            item.notes = notes;
        }
        // The answer's words join the reply's: both are the source.
        if item.quote.is_empty() {
            item.quote = quote;
        } else if !quote.is_empty() && !item.quote.contains(&quote) {
            item.quote = format!("{} … {}", item.quote, quote);
        }
        // EXCON decisions stand: an executed or ignored suggestion is not reopened
        // by a reading that says the same; an executed one is never changed.
        let same = item.params == params && item.role == role;
        match item.state {
            ItemState::Queued | ItemState::Executed | ItemState::Failed => {
                item.change = Change::Unchanged;
                if !same {
                    item.notes.push("A later answer suggests other parameters; this action was already executed.".into());
                }
                return Ok(());
            }
            ItemState::Ignored if same => {
                item.change = Change::Unchanged;
                return Ok(());
            }
            _ => {}
        }
        let was_incomplete = item.state == ItemState::Incomplete;
        let before = item.params.clone();
        if item.edited {
            // Keep what the EXCON typed; take from the AI only what is still missing.
            if let (Some(have), Some(new)) = (item.params.as_object_mut(), params.as_object()) {
                for (k, v) in new {
                    if have.get(k).is_none_or(|x| x.is_null() || x == "") {
                        have.insert(k.clone(), v.clone());
                    }
                }
            }
        } else {
            item.params = params;
            item.role = role;
        }
        if item.state == ItemState::Ignored {
            item.state = ItemState::Incomplete;
        }
        item.revision = revision;
        let changed = item.params != before;
        item.previous = changed.then_some(before);
        let complete = sim::missing_params(&item.kind, &item.params).is_empty();
        item.change = if first || !changed {
            Change::Unchanged
        } else if was_incomplete && complete {
            Change::Completed
        } else {
            Change::Changed
        };
        match item.change {
            Change::Completed => diff.completed.push(id),
            Change::Changed => diff.changed.push(id),
            _ => {}
        }
        Ok(())
    }

    /// Recompute completeness, the command and its feasibility for the open
    /// suggestions of `pid` (in order: each is checked after the earlier ones).
    fn recheck_items(&mut self, pid: u64) {
        let ids: Vec<u64> = self
            .review
            .items
            .iter()
            .filter(|i| {
                i.proposal == pid
                    && matches!(
                        i.state,
                        ItemState::Complete | ItemState::Incomplete | ItemState::Ignored
                    )
            })
            .map(|i| i.id)
            .collect();
        for id in ids {
            let item = self
                .review
                .items
                .iter()
                .find(|i| i.id == id)
                .cloned()
                .expect("item");
            let missing: Vec<String> = sim::missing_params(&item.kind, &item.params)
                .into_iter()
                .map(String::from)
                .collect();
            let (command, invalid) = if missing.is_empty() {
                match command_of(&item.kind, &item.params) {
                    Ok(c) => (Some(self.world.reconcile_command(&c).0), None),
                    Err(e) => (None, Some(e)),
                }
            } else {
                (None, None)
            };
            let verdict = command.as_ref().map(|c| self.review_verdict(&item.role, c));
            let description = match &command {
                Some(c) => c.describe(),
                None => describe_partial(&item.kind, &item.params, &missing),
            };
            let item = self
                .review
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .expect("item");
            if let Some(c) = &command {
                item.params = params_of(c);
            }
            item.missing = missing;
            item.invalid = invalid;
            item.verdict = verdict;
            item.description = description;
            if item.state != ItemState::Ignored {
                item.state = if command.is_some() {
                    ItemState::Complete
                } else {
                    ItemState::Incomplete
                };
            }
        }
    }

    /// Feasibility of `cmd` for `role`, after every approved or executed action.
    fn review_verdict(&self, role: &str, cmd: &Command) -> Verdict {
        let actor = Actor::Role(role.into());
        let mut v =
            self.world
                .validate_batch(&self.prior(u64::MAX), std::slice::from_ref(cmd), &actor);
        self.check_limit_batch(u64::MAX, std::slice::from_ref(cmd), &actor, &mut v);
        v.pop().unwrap_or(Verdict::Ok { notes: Vec::new() })
    }

    /// Replace the unsent drafts of `pid` with the reading's questions: one
    /// per player, bundling every gap. A gap the model asked nothing about gets
    /// a template question, so an incomplete suggestion is never left silent.
    fn merge_questions(
        &mut self,
        pid: u64,
        revision: u32,
        reading: &Reading,
        placed: &[Option<u64>],
        author: &str,
    ) -> Result<()> {
        for q in self
            .review
            .questions
            .iter_mut()
            .filter(|q| q.proposal == pid && q.state == QuestionState::Draft)
        {
            q.state = QuestionState::Discarded;
        }
        let mut drafts: Vec<(String, String, Vec<u64>, String, bool)> = Vec::new();
        for q in &reading.questions {
            let text: String = q.text.trim().chars().take(MAX_QUESTION).collect();
            if text.is_empty() || !self.players.contains_key(&q.to) {
                continue;
            }
            let about: Vec<u64> = q
                .about
                .iter()
                .filter_map(|i| placed.get(*i).copied().flatten())
                .collect();
            let reason: String = q.reason.trim().chars().take(300).collect();
            match drafts.iter_mut().find(|d| d.0 == q.to) {
                Some(d) => {
                    d.1 = format!("{} {}", d.1, text);
                    d.2.extend(about);
                    if !reason.is_empty() {
                        d.3 = format!("{} {}", d.3, reason).trim().to_string();
                    }
                }
                None => drafts.push((q.to.clone(), text, about, reason, false)),
            }
        }
        // Gaps nobody asks about yet, per player whose words they come from.
        let waiting = |to: &str, item: u64, qs: &[ReviewQuestion]| {
            qs.iter().any(|q| {
                q.proposal == pid
                    && q.to == to
                    && q.state == QuestionState::Sent
                    && q.about.contains(&item)
            })
        };
        let gaps: Vec<ReviewItem> = self
            .review
            .items
            .iter()
            .filter(|i| {
                i.proposal == pid && i.state == ItemState::Incomplete && !i.missing.is_empty()
            })
            .cloned()
            .collect();
        for item in gaps {
            let to = item.source_role.clone();
            if waiting(&to, item.id, &self.review.questions)
                || drafts.iter().any(|d| d.0 == to && d.2.contains(&item.id))
            {
                continue;
            }
            if let Some(d) = drafts.iter_mut().find(|d| d.0 == to) {
                // The model asked this player something else: the gap joins that question.
                d.2.push(item.id);
                continue;
            }
            drafts.push((to, String::new(), vec![item.id], String::new(), true));
        }
        for (to, text, mut about, reason, template) in drafts {
            about.sort_unstable();
            about.dedup();
            let language = self.language_of(&to);
            let text = if template {
                self.template_question(&about, &language)
            } else {
                text
            };
            let id = self.id();
            let persona = self.persona_for(None, self.proposal(pid).and_then(|p| p.inject_id));
            self.review.questions.push(ReviewQuestion {
                id,
                proposal: pid,
                cross_role: to != author,
                to,
                text,
                language,
                text_en: None,
                about,
                reason,
                template,
                persona,
                state: QuestionState::Draft,
                message: None,
                answer: None,
                revision,
                turn: self.turn,
            });
        }
        Ok(())
    }

    /// One question listing what each suggestion still needs (Italian for
    /// Italian players, English otherwise; the EXCON reviews it before sending).
    fn template_question(&self, about: &[u64], language: &str) -> String {
        let it = language == "it";
        let parts: Vec<String> = about
            .iter()
            .filter_map(|id| self.review.items.iter().find(|i| i.id == *id))
            .map(|i| {
                format!(
                    "{}: {}",
                    action_noun(&i.kind, it),
                    i.missing
                        .iter()
                        .map(|m| field_question(&i.kind, m, it))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect();
        if it {
            format!("Per procedere ci servono questi dettagli — {}. Come nella guida alla risposta: dove (zona come F4, un luogo o coordinate), con quali mezzi ed entro quando.", parts.join("; "))
        } else {
            format!("To proceed, please specify — {}. As in the reply guidance: where (zone like F4, a place or coordinates), with which resources and by when.", parts.join("; "))
        }
    }

    /// The EXCON edits a suggestion's parameters (and optionally its executing role).
    pub fn edit_review_item(&mut self, id: u64, role: Option<String>, params: Value) -> Result<()> {
        let Value::Object(mut o) = params else {
            bail!("parameters must be an object")
        };
        o.remove("type");
        o.retain(|_, v| !v.is_null() && v != "");
        if let Some(role) = &role {
            if !self.players.contains_key(role) {
                bail!("unknown role {role}");
            }
        }
        let item = self.item_mut(id)?;
        if matches!(item.state, ItemState::Queued | ItemState::Executed | ItemState::Failed) {
            bail!("suggestion {id} was already executed");
        }
        let before = item.params.clone();
        item.params = Value::Object(o);
        if let Some(role) = role {
            item.role = role;
        }
        item.edited = true;
        if item.state == ItemState::Ignored {
            item.state = ItemState::Incomplete;
        }
        let pid = item.proposal;
        let after = item.params.clone();
        self.recheck_items(pid);
        self.record(
            "review_item_edited",
            MASTER,
            json!({"item": id, "before": before, "after": after}),
        );
        Ok(())
    }

    /// The EXCON decides to run a complete suggestion (optionally with edited
    /// parameters): it becomes a command of its role, checked for that role.
    /// Like every EXCON action on the world it waits for play
    /// ([`Exercise::apply_or_queue_excon`]): with [`ExconWhen::Play`] or
    /// [`ExconWhen::At`] it is queued (state `Queued`, `None` returned) and runs
    /// as its role when time next moves; [`ExconWhen::Now`] applies it at once.
    pub fn execute_review_item(
        &mut self,
        id: u64,
        params: Option<Value>,
        role: Option<String>,
        when: ExconWhen,
    ) -> Result<Option<ExecutionRecord>> {
        if self.phase != Phase::Orders {
            bail!("actions are executed while the turn is open");
        }
        if params.is_some() || role.is_some() {
            let current = self
                .review
                .items
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.params.clone())
                .unwrap_or_default();
            self.edit_review_item(id, role, params.unwrap_or(current))?;
        }
        let (item, cmd) = self.runnable_review_item(id)?;
        if when == ExconWhen::Now {
            return self.apply_review_item(id).map(Some);
        }
        let now = self.world.time_s();
        let run_at_s = match when {
            ExconWhen::At(t) if t < now => bail!("the chosen time has already passed"),
            ExconWhen::At(t) => t,
            _ => now,
        };
        let origin = json!({"origin": "excon_review", "review_item": id, "role": item.role});
        let queued = self.enqueue_excon(&cmd, run_at_s, matches!(when, ExconWhen::Play), origin)?;
        let it = self.item_mut(id)?;
        it.state = ItemState::Queued;
        it.queued = Some(queued);
        it.params = params_of(&cmd);
        it.description = cmd.describe();
        self.record(
            "review_item_queued",
            MASTER,
            json!({"review_item": id, "proposal": item.proposal, "role": item.role, "queue_id": queued, "run_at_s": run_at_s}),
        );
        Ok(None)
    }

    /// The suggestion and its command, if it may run now as its role.
    fn runnable_review_item(&self, id: u64) -> Result<(ReviewItem, Command)> {
        let item = self
            .review
            .items
            .iter()
            .find(|i| i.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown suggestion {id}"))?;
        match item.state {
            ItemState::Queued | ItemState::Executed | ItemState::Failed => {
                bail!("suggestion {id} was already executed")
            }
            ItemState::Incomplete if !item.missing.is_empty() => {
                bail!("suggestion {id} still needs: {}", item.missing.join(", "))
            }
            _ => {}
        }
        let cmd = command_of(&item.kind, &item.params).map_err(anyhow::Error::msg)?;
        let (cmd, _) = self.world.reconcile_command(&cmd);
        // Not feasible now: nothing runs, the suggestion stays open to edit.
        if let Verdict::Rejected { reason } = self.review_verdict(&item.role, &cmd) {
            bail!("not feasible: {reason}");
        }
        Ok((item, cmd))
    }

    /// Apply a suggestion as its role, now: at once ([`ExconWhen::Now`]) or
    /// when its queued action is due. A queued one that is no longer feasible
    /// fails with the simulator's reason.
    pub(crate) fn apply_review_item(&mut self, id: u64) -> Result<ExecutionRecord> {
        let item = self
            .review
            .items
            .iter()
            .find(|i| i.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown suggestion {id}"))?;
        let cmd = command_of(&item.kind, &item.params).map_err(anyhow::Error::msg)?;
        let (cmd, _) = self.world.reconcile_command(&cmd);
        let actor = Actor::Role(item.role.clone());
        let allocations = self.world.d.allocations.len();
        let events = self.world.d.resource_events.len();
        let result = match self.review_verdict(&item.role, &cmd) {
            Verdict::Rejected { reason } => Err(format!("not feasible: {reason}")),
            _ => self.world.apply(&cmd, &actor),
        };
        let (ok, detail) = match result {
            Ok(notes) => (true, notes.join("; ")),
            Err(reason) => (false, reason),
        };
        self.record_resource_events(events, &item.role);
        self.absorb_monitoring();
        let execution = ExecutionRecord {
            allocations: self.world.d.allocations[allocations..]
                .iter()
                .map(|a| a.id)
                .collect(),
            command: cmd.clone(),
            ok,
            detail,
            t_s: self.world.time_s(),
            wall_ms: crate::wall_ms(),
            origin: "excon_review".into(),
        };
        let it = self.item_mut(id)?;
        it.state = if ok {
            ItemState::Executed
        } else {
            ItemState::Failed
        };
        it.execution = Some(execution.clone());
        it.params = params_of(&cmd);
        it.description = cmd.describe();
        self.record(
            "action_executed",
            MASTER,
            json!({"review_item": id, "proposal": item.proposal, "role": item.role, "source_role": item.source_role, "quote": item.quote, "execution": execution, "queue_id": item.queued}),
        );
        // Later suggestions are checked against what is now committed.
        self.recheck_items(item.proposal);
        Ok(execution)
    }

    /// Keep suggestions in step with their queued actions after a cancel or a
    /// rollback: still waiting → `Queued`; withdrawn or forgotten → back to
    /// `Complete` (to run again or ignore).
    pub(crate) fn sync_review_queue(&mut self) {
        let runs: Vec<(u64, Option<crate::actions::ActionStatus>)> = self
            .review
            .items
            .iter()
            .filter_map(|i| i.queued)
            .map(|q| (q, self.action_runs.get(&format!("excon:{q}")).map(|r| r.status)))
            .collect();
        for (q, status) in runs {
            let Some(it) = self.review.items.iter_mut().find(|i| i.queued == Some(q)) else { continue };
            match status {
                Some(crate::actions::ActionStatus::Approved) => {
                    it.state = ItemState::Queued;
                    it.execution = None;
                }
                Some(crate::actions::ActionStatus::Done | crate::actions::ActionStatus::Failed) => {}
                _ => {
                    it.state = ItemState::Complete;
                    it.execution = None;
                    it.queued = None;
                }
            }
        }
    }

    /// The EXCON decides not to act on a suggestion (it stays listed).
    pub fn ignore_review_item(&mut self, id: u64, note: Option<String>) -> Result<()> {
        let item = self.item_mut(id)?;
        if matches!(item.state, ItemState::Queued | ItemState::Executed | ItemState::Failed) {
            bail!("suggestion {id} was already executed");
        }
        item.state = ItemState::Ignored;
        item.note = note.map(|n| n.chars().take(500).collect());
        self.record("review_item_ignored", MASTER, json!({"item": id}));
        Ok(())
    }

    /// Undo an "ignore".
    pub fn restore_review_item(&mut self, id: u64) -> Result<()> {
        let item = self.item_mut(id)?;
        if item.state != ItemState::Ignored {
            bail!("suggestion {id} is not ignored");
        }
        item.state = ItemState::Incomplete;
        let pid = item.proposal;
        self.recheck_items(pid);
        self.record("review_item_restored", MASTER, json!({"item": id}));
        Ok(())
    }

    /// The EXCON writes a question of its own (a draft, reviewed like the AI's).
    pub fn add_review_question(&mut self, pid: u64, to: &str, text: &str) -> Result<u64> {
        let r = self
            .review(pid)
            .ok_or_else(|| anyhow::anyhow!("reply {pid} has no review"))?;
        if !self.players.contains_key(to) {
            bail!("unknown role {to}");
        }
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_QUESTION {
            bail!("the question needs 1–{MAX_QUESTION} characters");
        }
        let (author, revision) = (r.role.clone(), r.revision);
        let id = self.id();
        let persona = self.persona_for(None, self.proposal(pid).and_then(|p| p.inject_id));
        self.review.questions.push(ReviewQuestion {
            id,
            proposal: pid,
            to: to.into(),
            text: text.into(),
            language: self.language_of(to),
            text_en: None,
            about: Vec::new(),
            cross_role: to != author,
            reason: String::new(),
            template: false,
            persona,
            state: QuestionState::Draft,
            message: None,
            answer: None,
            revision,
            turn: self.turn,
        });
        self.record(
            "review_question_drafted",
            MASTER,
            json!({"question": id, "proposal": pid, "to": to}),
        );
        Ok(id)
    }

    /// What an EXCON-reviewed draft will say: `(recipient, text in the
    /// recipient's language, persona)`, with the EXCON's edits applied.
    /// Nothing is sent: the caller makes the English copy, then calls
    /// [`Exercise::send_review_question`].
    pub fn review_question_to_send(
        &self,
        qid: u64,
        text: Option<&str>,
        persona: Option<&str>,
    ) -> Result<(String, String, Option<String>)> {
        let q = self
            .review
            .questions
            .iter()
            .find(|q| q.id == qid)
            .ok_or_else(|| anyhow::anyhow!("unknown question {qid}"))?;
        if q.state != QuestionState::Draft {
            bail!("question {qid} is not a draft");
        }
        if self.phase != Phase::Orders {
            bail!("questions are sent while the turn is open");
        }
        let text = text.unwrap_or(&q.text).trim().to_string();
        if text.is_empty() || text.chars().count() > MAX_QUESTION {
            bail!("the question needs 1–{MAX_QUESTION} characters");
        }
        let inject = self.proposal(q.proposal).and_then(|p| p.inject_id);
        Ok((
            q.to.clone(),
            text,
            self.persona_for(persona, inject).or(q.persona.clone()),
        ))
    }

    /// The EXCON sends a reviewed draft: `text` exactly as reviewed, in the
    /// player's language, with its English copy. The only way a question leaves.
    pub fn send_review_question(
        &mut self,
        qid: u64,
        text: &str,
        text_en: &str,
        persona: Option<String>,
    ) -> Result<u64> {
        let (to, _, _) = self.review_question_to_send(qid, Some(text), persona.as_deref())?;
        let pid = self
            .review
            .questions
            .iter()
            .find(|q| q.id == qid)
            .map(|q| q.proposal)
            .expect("question");
        let p = self
            .proposal(pid)
            .ok_or_else(|| anyhow::anyhow!("unknown reply {pid}"))?;
        // In the inject's thread of the recipient, when they received it.
        let thread = p.inject_id.map(|root| (root, None, vec![to.clone()]));
        let reply_to = if to == p.source.label() {
            self.messages
                .iter()
                .find(|m| m.proposal == Some(pid) && m.kind == MsgKind::Reply)
                .map(|m| m.id)
        } else {
            None
        };
        let mid = self.add_message_in_thread(
            MASTER,
            &to,
            MsgKind::Clarification,
            Some("Clarification requested"),
            text,
            text_en,
            None,
            thread,
            reply_to,
        );
        let persona = persona.or_else(|| {
            self.review
                .questions
                .iter()
                .find(|q| q.id == qid)
                .and_then(|q| q.persona.clone())
        });
        let who = self.persona_name(persona.as_deref(), "en");
        self.set_message_persona(mid, persona.clone());
        let q = self.question_mut(qid)?;
        q.state = QuestionState::Sent;
        q.text_en = Some(text_en.trim().into());
        q.text = text.trim().into();
        q.message = Some(mid);
        q.persona = persona;
        self.converse(
            &to,
            "user",
            format!("[{}] QUESTION from {who}:\n{text_en}", self.world.clock()),
        );
        self.record(
            "review_question_sent",
            MASTER,
            json!({"question": qid, "proposal": pid, "to": to, "message": mid}),
        );
        Ok(mid)
    }

    /// The EXCON discards a draft (or withdraws a question not yet answered).
    pub fn discard_review_question(&mut self, qid: u64) -> Result<()> {
        let q = self.question_mut(qid)?;
        if !matches!(q.state, QuestionState::Draft | QuestionState::Sent) {
            bail!("question {qid} is already {:?}", q.state);
        }
        q.state = QuestionState::Discarded;
        self.record(
            "review_question_discarded",
            MASTER,
            json!({"question": qid}),
        );
        Ok(())
    }

    /// The sent question of `role` that message `reply_to` is, still unanswered.
    pub fn review_question_for(&self, role: &str, reply_to: u64) -> Option<u64> {
        self.review
            .questions
            .iter()
            .find(|q| q.to == role && q.message == Some(reply_to) && q.state == QuestionState::Sent)
            .map(|q| q.id)
    }

    /// A player answers a question (free text, translated by the caller). The
    /// answer is linked to it; the caller then has the thread read again
    /// ([`Exercise::begin_review`]). Returns `(message, reply)`.
    pub fn answer_review_question(
        &mut self,
        role: &str,
        text: &str,
        reply_to: u64,
        text_en: &str,
        translation_failed: bool,
    ) -> Result<(u64, u64)> {
        if self.phase != Phase::Orders {
            bail!("no turn is open for answers");
        }
        if text.trim().is_empty() {
            bail!("empty answer");
        }
        let Some(qid) = self.review_question_for(role, reply_to) else {
            bail!("there is no open question to answer");
        };
        let pid = self
            .review
            .questions
            .iter()
            .find(|q| q.id == qid)
            .map(|q| q.proposal)
            .expect("question");
        let root = self
            .messages
            .iter()
            .find(|m| m.id == reply_to)
            .and_then(|m| m.inject_id);
        let mid = self.add_message_in_thread(
            role,
            MASTER,
            MsgKind::Reply,
            None,
            text,
            text_en,
            None,
            root.map(|r| (r, Some(reply_to), vec![MASTER.into()])),
            Some(reply_to),
        );
        let language = self.language_of(role);
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == mid) {
            m.original = Some(text.into());
            m.language_src = Some(language);
            m.translation_failed = translation_failed;
        }
        let q = self.question_mut(qid)?;
        q.state = QuestionState::Answered;
        q.answer = Some(mid);
        self.converse(
            role,
            "user",
            format!(
                "[{}] ANSWER to the question:\n{text_en}",
                self.world.clock()
            ),
        );
        self.record(
            "review_question_answered",
            role,
            json!({"question": qid, "proposal": pid, "message": mid}),
        );
        // A closed review reopens: there is something new to read.
        if let Some(r) = self.review.reviews.iter_mut().find(|r| r.proposal == pid) {
            if r.status == ReviewStatus::Closed {
                r.status = ReviewStatus::Ready;
            }
        }
        Ok((mid, pid))
    }

    /// The EXCON closes the review of `pid`: unsent drafts are discarded,
    /// open suggestions and unanswered questions stay listed.
    pub fn close_review(&mut self, pid: u64) -> Result<()> {
        let r = self.review_mut(pid)?;
        r.status = ReviewStatus::Closed;
        for q in self
            .review
            .questions
            .iter_mut()
            .filter(|q| q.proposal == pid && q.state == QuestionState::Draft)
        {
            q.state = QuestionState::Discarded;
        }
        self.record("review_closed", MASTER, json!({"proposal": pid}));
        Ok(())
    }

    /// Undo the reviews of rolled-back turns (the world is restored).
    pub(crate) fn rollback_reviews(&mut self, turn: u32) {
        let gone: Vec<u64> = self
            .review
            .reviews
            .iter()
            .filter(|r| r.turn >= turn)
            .map(|r| r.proposal)
            .collect();
        self.review.reviews.retain(|r| r.turn < turn);
        self.review
            .items
            .retain(|i| i.turn < turn && !gone.contains(&i.proposal));
        self.review
            .questions
            .retain(|q| q.turn < turn && !gone.contains(&q.proposal));
    }

    /// Counts for the turn panel: open questions (drafts to review and
    /// questions awaiting an answer) and suggestions waiting for a decision.
    pub fn review_counts(&self) -> Value {
        let q = |s: QuestionState| {
            self.review
                .questions
                .iter()
                .filter(|q| q.state == s)
                .count()
        };
        let i = |s: ItemState| self.review.items.iter().filter(|i| i.state == s).count();
        let infeasible = self
            .review
            .items
            .iter()
            .filter(|i| i.state == ItemState::Complete && matches!(i.verdict, Some(Verdict::Rejected { .. })))
            .count();
        json!({
            "drafts": q(QuestionState::Draft),
            "awaiting_answer": q(QuestionState::Sent),
            // Executable now; complete but refused by the simulator is "infeasible".
            "complete": i(ItemState::Complete) - infeasible,
            "infeasible": infeasible,
            "incomplete": i(ItemState::Incomplete),
            "reading": self.review.reviews.iter().filter(|r| r.status == ReviewStatus::Reading).count(),
        })
    }

    /// The board as the EXCON console shows it, newest reply first. Each
    /// suggestion carries a `display` state: complete, infeasible, incomplete,
    /// question_draft, awaiting_answer, executed, failed or ignored.
    pub fn review_view(&self) -> Value {
        let reviews = self
            .review
            .reviews
            .iter()
            .rev()
            .map(|r| {
                let p = self.proposal(r.proposal);
                let questions: Vec<&ReviewQuestion> = self.review.questions.iter().filter(|q| q.proposal == r.proposal).collect();
                let items = self
                    .review
                    .items
                    .iter()
                    .filter(|i| i.proposal == r.proposal)
                    .map(|i| {
                        let display = match i.state {
                            ItemState::Incomplete if questions.iter().any(|q| q.state == QuestionState::Sent && q.about.contains(&i.id)) => "awaiting_answer",
                            ItemState::Incomplete if questions.iter().any(|q| q.state == QuestionState::Draft && q.about.contains(&i.id)) => "question_draft",
                            // Every parameter known, but the simulator refuses it now.
                            ItemState::Complete if matches!(i.verdict, Some(Verdict::Rejected { .. })) => "infeasible",
                            ItemState::Complete => "complete",
                            ItemState::Incomplete => "incomplete",
                            ItemState::Queued => "queued",
                            ItemState::Executed => "executed",
                            ItemState::Failed => "failed",
                            ItemState::Ignored => "ignored",
                        };
                        let mut v = json!(i);
                        v["display"] = display.into();
                        v
                    })
                    .collect::<Vec<_>>();
                let questions = questions
                    .iter()
                    .map(|q| {
                        let mut v = json!(q);
                        v["answer_text"] = q.answer.and_then(|id| self.messages.iter().find(|m| m.id == id)).map(|m| json!({"text": m.text_en, "original": m.original, "clock": m.clock})).unwrap_or(Value::Null);
                        v["sent_clock"] = q.message.and_then(|id| self.messages.iter().find(|m| m.id == id)).map(|m| json!(m.clock)).unwrap_or(Value::Null);
                        v["persona_name"] = json!(self.persona_name(q.persona.as_deref(), &self.language_of(MASTER)));
                        v
                    })
                    .collect::<Vec<_>>();
                let inject = r.inject_id.and_then(|root| self.messages.iter().find(|m| m.inject_id == Some(root) && m.kind == MsgKind::Inject));
                json!({
                    "proposal": r.proposal,
                    "role": r.role,
                    "inject_id": r.inject_id,
                    "inject_title": inject.and_then(|m| m.title.clone()),
                    "inject_clock": inject.map(|m| m.clock.clone()),
                    "reply": p.map(|p| p.original.clone()),
                    "reply_en": p.map(|p| p.translation_en.clone()),
                    "reply_clock": p.map(|p| scenario::format_clock(self.world.scn.start_clock_s + p.t_s)),
                    "basis": self.basis_of(Some(r.proposal)),
                    "basis_note": self.basis_note_of(Some(r.proposal)),
                    "revision": r.revision,
                    "status": r.status,
                    "summary": r.summary,
                    "error": r.error,
                    "diff": r.diff,
                    "turn": r.turn,
                    "items": items,
                    "questions": questions,
                })
            })
            .collect::<Vec<_>>();
        json!({"reviews": reviews, "counts": self.review_counts()})
    }

    /// Questions sent to `role`, as clarification threads of the player page
    /// (`review: true`): the question, then the answer once given.
    pub fn player_review_threads(&self, role: &str) -> Vec<Value> {
        let language = self.language_of(role);
        self.review
            .questions
            .iter()
            .filter(|q| q.to == role && matches!(q.state, QuestionState::Sent | QuestionState::Answered) && q.message.is_some())
            .filter_map(|q| {
                let m = self.messages.iter().find(|m| Some(m.id) == q.message)?;
                let mut entries = vec![json!({"seq": 0, "author": "excon", "from": MASTER, "text": m.text, "message": m.id,
                    "persona": self.persona_name(m.persona.as_deref(), &language), "proposal": Value::Null, "turn": m.turn, "t_s": m.t_s, "clock": m.clock})];
                if let Some(a) = q.answer.and_then(|id| self.messages.iter().find(|x| x.id == id)) {
                    entries.push(json!({"seq": 1, "author": "player", "from": role, "text": a.original.clone().unwrap_or_else(|| a.text.clone()),
                        "message": a.id, "proposal": Value::Null, "turn": a.turn, "t_s": a.t_s, "clock": a.clock}));
                }
                Some(json!({
                    "id": q.id, "review": true, "role": role, "inject_id": m.inject_id,
                    "status": if q.state == QuestionState::Sent { "awaiting_player" } else { "closed" },
                    "answered": q.state == QuestionState::Answered,
                    "entries": entries,
                }))
            })
            .collect()
    }
}

/// The command a suggestion stands for, or why its values are not valid.
pub fn command_of(kind: &str, params: &Value) -> std::result::Result<Command, String> {
    let mut v = match params {
        Value::Object(o) => Value::Object(o.clone()),
        _ => json!({}),
    };
    v["type"] = kind.into();
    // Numbers typed as text in the console ("2") are accepted.
    if let Some(o) = v.as_object_mut() {
        for key in [
            "qty",
            "width_m",
            "radius_m",
            "minutes",
            "delta",
            "allocation",
            "speed_mps",
        ] {
            if let Some(Value::String(s)) = o.get(key) {
                if let Ok(n) = s.trim().parse::<f64>() {
                    o.insert(key.into(), json!(n));
                }
            }
        }
        for key in ["qty", "minutes", "allocation"] {
            if let Some(n) = o.get(key).and_then(Value::as_f64) {
                o.insert(key.into(), json!(n.max(0.0) as u64));
            }
        }
        if let Some(n) = o.get("delta").and_then(Value::as_f64) {
            o.insert("delta".into(), json!(n as i64));
        }
        if let Some(Value::String(s)) = o.get("zones") {
            let zones: Vec<String> = s
                .split(',')
                .map(|z| z.trim().to_string())
                .filter(|z| !z.is_empty())
                .collect();
            o.insert("zones".into(), json!(zones));
        }
    }
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// The command's fields without `"type"`.
fn params_of(c: &Command) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.remove("type");
    }
    v
}

/// "CutFireLine (from ?, to ?)" for an incomplete suggestion (EXCON only).
fn describe_partial(kind: &str, params: &Value, missing: &[String]) -> String {
    let known = params
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| {
                    format!(
                        "{k} {}",
                        v.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| v.to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let gaps = missing
        .iter()
        .map(|m| format!("{m} ?"))
        .collect::<Vec<_>>()
        .join(", ");
    match (known.is_empty(), gaps.is_empty()) {
        (true, _) => format!("{kind} ({gaps})"),
        (false, true) => format!("{kind} ({known})"),
        (false, false) => format!("{kind} ({known}; {gaps})"),
    }
}

/// What a template question calls the action, in Italian or English.
fn action_noun(kind: &str, it: bool) -> &'static str {
    let (i, e) = match kind {
        "CutFireLine" => ("la linea tagliafuoco", "the fire line"),
        "WaterDrop" => ("il lancio d’acqua", "the water drop"),
        "DeployEngine" => ("l’invio della squadra", "the team deployment"),
        "MonitorArea" => ("il monitoraggio con drone", "the drone monitoring"),
        "Reconnoitre" => ("la ricognizione", "the reconnaissance"),
        "EvacuateZone" => ("l’evacuazione", "the evacuation"),
        "EvacuateBuilding" => ("l’evacuazione dell’edificio", "the building evacuation"),
        "IssueWarning" => ("l’avviso", "the warning"),
        "CloseRoad" => ("la chiusura della strada", "the road closure"),
        "ReopenRoad" => ("la riapertura della strada", "the road reopening"),
        "OpenShelter" => ("il punto di accoglienza", "the shelter"),
        "RequestReinforcement" => ("la richiesta di rinforzi", "the reinforcement request"),
        _ => ("l’azione", "the action"),
    };
    if it {
        i
    } else {
        e
    }
}

/// What a template question asks for a missing field.
fn field_question(kind: &str, field: &str, it: bool) -> &'static str {
    let (i, e) = match (kind, field) {
        ("CutFireLine", "from") => ("tra quali punti (inizio)", "between which points (start)"),
        ("CutFireLine", "to") => ("tra quali punti (fine)", "between which points (end)"),
        (_, "at") => ("dove esattamente", "where exactly"),
        (_, "zone") | (_, "zones") => ("quale zona", "which zone"),
        (_, "road") => ("quale strada", "which road"),
        (_, "building") => ("quale edificio", "which building"),
        (_, "resource") => ("quale risorsa", "which resource"),
        (_, "qty") => ("quanti", "how many"),
        _ => ("i dettagli mancanti", "the missing details"),
    };
    if it {
        i
    } else {
        e
    }
}
