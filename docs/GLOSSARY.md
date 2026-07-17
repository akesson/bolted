# Bolted — Glossary

The **ubiquitous language** of Bolted, in the domain-driven-design sense: a deliberately
small, curated vocabulary whose words carry exact meaning everywhere — docs, code, commit
messages, conversation.

Rules:

- A term enters this file **only by explicit decision of the project owner**. Propose and
  ask; never add unilaterally.
- Definitions are self-contained prose. No references to other documents or sections — if a
  definition can't stand alone, the term isn't ready.
- When a term's meaning shifts, its definition changes in the same commit as the design.

---

## Facet

A **facet** is the unit a Bolted core exports to its shells: a domain-grouped, reactive set
of state and operations — one deliberately cut face of the domain, made to be observed.

A facet is scoped by domain cohesion, never by view structure. A screen may compose several
facets; a tray icon may observe a sliver of one. Bolted ships no ViewModels: whatever
view-scoped grouping an app wants is the app's own composition over facets.
