# Contributing to ApexNorth

Thanks for helping make ApexNorth better. Bug reports, ideas, lap data from tracks we don't recognise yet, and pull requests are all welcome.

## Before you start

- **Bugs and ideas:** open an [issue](https://github.com/ElevenNorthCo/ApexNorth/issues). For a bug, say what you were doing in GT7, your OS, and what the app showed. A screenshot helps.
- **Bigger changes:** open an issue first so we can agree on the approach before you spend time on it.
- Check the [roadmap](README.md#roadmap-phases--milestones) to see what's planned.

## Making a change

1. Fork the repo and create a branch.
2. Set up as in [Development](README.md#development). **Try demo mode** on the Connect screen lets you test without a PlayStation.
3. Keep the change focused, and match the style of the code around it.
4. Before opening a pull request:
   ```bash
   cd src-tauri && cargo test
   npx tsc --noEmit
   ```
5. Open the pull request and describe what changed and how you tested it.

Every pull request is reviewed before it's merged.

## License of contributions

ApexNorth is free for noncommercial use under the [PolyForm Noncommercial License 1.0.0](LICENSE), and Eleven North keeps the right to offer it (or paid features built on it, such as a hosted AI engineer) under other terms. So that stays possible, contributions need this grant:

> By submitting a contribution (code, documentation, data, artwork or anything else) to this repository, you confirm that you wrote it or otherwise have the right to submit it, and you grant Eleven North a perpetual, worldwide, non-exclusive, royalty-free, irrevocable license to use, copy, modify, distribute, sublicense and relicense your contribution, under any terms, including commercial ones. You keep the copyright in your contribution, and you can use your own contribution however you like.

The pull request template asks you to tick a box agreeing to this. Please don't submit code you copied from somewhere else unless its license allows it, and say where it came from.

## Code of conduct

Be kind and constructive. Harassment or abuse gets you removed from the project.
