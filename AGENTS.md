# AGENTS.md

Rules for anyone, human or agent, who changes this repository. Build and CI
commands for agents are in [docs/agenter.md](docs/agenter.md).

## Reuse before you build (GPUI app)

[docs/design.md](docs/design.md) lists the foundation, every component in
`crates/tingskriver-app`, the product concepts and the experiences.

1. Find the existing component and a screenshot of a similar screen first.
2. Create a new component only if nothing covers the need; if one lacks a
   state, extend it.
3. Check the consumers (`git grep -n "<name>(" crates/`) before you change a
   shared component.
4. Update `docs/design.md` in the same change if a component's
   responsibility or an experience changes. `python3 scripts/check-design-doc.py`
   enforces the component list.
5. Answer "reused or new component, and why" in the pull request template.
