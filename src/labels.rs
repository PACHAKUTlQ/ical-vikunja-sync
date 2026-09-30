use crate::{
    config::{FeedConfig, LabelRule, MatchField, MatchMode},
    ical::Occurrence,
    vikunja::Vikunja,
};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};

struct ResolvedRule<'a> {
    rule: &'a LabelRule,
    label_id: i64,
}

pub struct LabelPolicy<'a> {
    rules: Vec<ResolvedRule<'a>>,
    managed: BTreeSet<i64>,
}

impl<'a> LabelPolicy<'a> {
    pub async fn resolve(api: &Vikunja, feed: &'a FeedConfig) -> Result<Self> {
        if feed.label_rules.is_empty() {
            return Ok(Self {
                rules: Vec::new(),
                managed: BTreeSet::new(),
            });
        }

        let configured_titles: BTreeSet<&str> = feed
            .label_rules
            .iter()
            .map(|rule| rule.label.as_str())
            .collect();

        let mut existing = BTreeMap::<String, BTreeSet<i64>>::new();
        for label in api.labels().await.context("cannot list Vikunja labels")? {
            if configured_titles.contains(label.title.as_str()) {
                existing.entry(label.title).or_default().insert(label.id);
            }
        }

        for (title, ids) in &existing {
            if ids.len() > 1 {
                bail!(
                    "multiple accessible Vikunja labels have title {title:?}; \
                     label titles used by this feed must resolve uniquely"
                );
            }
        }

        let mut resolved = BTreeMap::<&str, i64>::new();
        for title in configured_titles {
            let id = match existing.get(title).and_then(|ids| ids.first()).copied() {
                Some(id) => id,
                None => api
                    .create_label(title)
                    .await
                    .with_context(|| format!("cannot create Vikunja label {title:?}"))?,
            };
            resolved.insert(title, id);
        }

        let rules = feed
            .label_rules
            .iter()
            .map(|rule| ResolvedRule {
                rule,
                label_id: resolved[rule.label.as_str()],
            })
            .collect();
        let managed = resolved.into_values().collect();

        Ok(Self { rules, managed })
    }

    pub async fn reconcile(&self, api: &Vikunja, task_id: i64, event: &Occurrence) -> Result<()> {
        if self.managed.is_empty() {
            return Ok(());
        }

        let desired = self.desired(event);
        let attached: BTreeSet<i64> = api
            .task_labels(task_id)
            .await
            .with_context(|| format!("cannot list labels on task {task_id}"))?
            .into_iter()
            .map(|label| label.id)
            .collect();

        for &label_id in desired.difference(&attached) {
            api.attach_label(task_id, label_id)
                .await
                .with_context(|| format!("cannot attach label {label_id} to task {task_id}"))?;
        }

        for &label_id in attached.intersection(&self.managed) {
            if !desired.contains(&label_id) {
                api.detach_label(task_id, label_id).await.with_context(|| {
                    format!("cannot detach label {label_id} from task {task_id}")
                })?;
            }
        }

        Ok(())
    }

    fn desired(&self, event: &Occurrence) -> BTreeSet<i64> {
        self.rules
            .iter()
            .filter(|resolved| matches(resolved.rule, event))
            .map(|resolved| resolved.label_id)
            .collect()
    }
}

fn matches(rule: &LabelRule, event: &Occurrence) -> bool {
    let mut matches = rule.fields.iter().map(|field| {
        let text = match field {
            MatchField::Summary => event.title.as_str(),
            MatchField::Description => event.description.as_str(),
            MatchField::Location => event.location.as_str(),
        };
        rule.pattern.is_match(text)
    });

    match rule.mode {
        MatchMode::Or => matches.any(|matched| matched),
        MatchMode::And => matches.all(|matched| matched),
    }
}
