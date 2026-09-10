# Branch cleanup ledger — 2026-07-28

Deleted local (and where noted, remote) branches during the post-#102 sprawl
cleanup. All were verified against `origin/main @ f4d9f02` (PR #102, the
superset): `git diff --diff-filter=A --name-only origin/main..<branch>` was
empty for every branch below (no file unique to the branch) — the sole
exception was `docs/c3-scope`, whose one unique file `C3_SCOPE.md` was
rescued into this commit.

To recover any branch: `git branch <name> <sha>` (SHAs also live in reflog
and, for pushed branches, the remote's object store).

```
chore/a1-close-and-ci-vitest                                    4d4ef08aa80c1e78e97540d54ee845ed297e56e3
chore/repin-sbt-4ce3                                            6c8249b81db37c798262070a70e99e0329c3d1b5
docs/b1-0-close                                                 a06d9524c489c4e8ff9ee3066c0883bbc9138d7b
docs/b1-1-0-done                                                cd18d5b057c8ac3003d37edc2cd5f86c74ddd0b0
docs/b1-1-scope                                                 b88ac4c53df7fa8fdd539bb55f49107658f8a2e9
docs/b1-2-done                                                  af40ff28b39245377076c5405dd4a1a536d4a511
docs/b1-2-scope                                                 ecaf2fc32b631195e95d842327a43bb52f955c65
docs/b1-3-scope                                                 3159d82ef7612215881895d118db4350845802d9
docs/b1-4-scope                                                 761c5519c857b469bf80022a1be839995000257a
docs/b1-5-scope                                                 92df13c105f6dcea67100b85fa3fc0dc7f71b0c4
docs/c1-1-scope                                                 e46bc476b7bb1822fbe0897b2987b74b3bbb2aef
docs/c1-2-scope                                                 514606961fc4885c218a089bd9a8e93b2f0d2401
docs/c1-scope                                                   9b947751af8f148665ac045f67d5c2c2e1353021
docs/c2-scope                                                   4b73f786a13d35b07e6ee917060d4a4db0d5a693
docs/c3-scope                                                   052ab6f123db3612a0cd87ac540c6cc47c08dd18
docs/phase-c-close                                              d400235856e9b850c509173c6c07cdb285460dc6
docs/phase-d-scope                                              027b54f61dca7fac29781cc5d0c60aa81696976e
docs/skill-artifacts                                            6d5ab75f5c253c451258596b289fe53328da4ef3
docs/sprint-a1-bridge                                           eee69077906e467f111cbdbf2c01c36ed37531ec
docs/sprint-a2-custody                                          91f8d78cf9a3a5034aeed0bfae6fe5944881fd20
docs/sprint-a3-oidc                                             d2ed45871b9a3662864cd190bf9cd6b6e66e8453
docs/sprint-b1-keystore                                         45e44317b0157d72ebb6a45118e7084c421014fd
feat/bundle-ipfs-daemon                                         29540b590a56483f2b8e709c17cf01b82f5dd8d5
feat/chain-realign-wp11                                         a2b09ed7bd66e4df7ae5cec1144aeff4958d840e
feat/core-a1-bridge                                             710c3cc6190b7336c735caf05ecc79cd06af471e
feat/core-a2-custody                                            a9ba9c65b80373fbc70ac6aba9bb8518047a3411
feat/core-a3-oidc                                               b30aaebbd4e55527161f46f3a20253144ad0dd46
feat/core-b1-0-anchor-init                                      92ccf95e578b6e6b0fa41361d08bd9dfff5de24b
feat/core-b1-0b-legacy-anchor                                   d5eaa82823425a5868d80ada420aa6c8d04cd160
feat/core-b1-1-keystore                                         6433fc5895a87e6223028f2417ca9314c1830ec3
feat/core-b1-2-ceremony                                         6d955495893cf58a221ceba34a40a623cc3ec94d
feat/core-b1-3-connector                                        bdbd2f0f0357b1a165ecb8ebdf12ee0b3833fb10
feat/core-b1-4-broadcast                                        4c97d36742bdcd1b6b1eace6ddb33b4e0b316ade
feat/core-b1-5-adversarial                                      22ce9c10bd750e3000f14a73850d323c3829796f
feat/core-c1-0-supervisor                                       ddc690b4d00b54c513545117ebb7b5d7e9950b75
feat/core-c1-0b-supervisor-formal                               40e665eaedc05882bba143737413c7bc0f4e5aed
feat/core-c1-1-node                                             b666c71675a989f9e4973105273549db12d60c36
feat/core-c1-2-node-agent                                       11fcb42aae808469260c489502f4f28d7fffc937
feat/core-c2-earnings                                           3c2b78def3aee1a97413b78f2c8c74e0b0160703
feat/core-c2-remediation                                        3e0a0fa20339b0ab388969e5a95f7f2587ceb197
feat/core-c3-memory                                             3e0a0fa20339b0ab388969e5a95f7f2587ceb197
feat/core-d3-0-popup-webview-auth                               f3e4f445aa4f8eb3ce0e6c510c04c3f9a21cefa5
feat/core-d3-c-onboarding-checkout                              627fe2fc17d43d94b66d0d9639cac0dd2a426e62
feat/core-frontend-1to1                                         d266b41b97641f37ef9a886cfcde964f1e9394aa
feat/core-s0-scaffold                                           1a86c6bb711bfdf4ff4654ce0a0ee4f4c7ed063b
feat/w1-3-validator-registration                                5885b384a3b6a625b105e50ae3a4524d2f2f872e
feat/w1-validator-produce-earn                                  6b0762ac1d4679158834c046c6562f37a17dbf30
feat/w2-in-app-auto-updates                                     8aef90adb7f98eceeac05df3fd33308ee3d10fb1
feat/w3-real-agent                                              429f58178f7b9d9d1fbcbf11fa2361c579640a8b
feat/w4-mcp-oauth                                               9d2081ddb49a27cb181b818313fbf42f48d3fff5
fix/cl-c2-resync-sbt                                            cbaa60536f92dd5585148615261202000674bc84
fix/core-d3-0-popup-main-thread                                 9500b48683df9b4249438ee196886b3e3fa87d3d
fix/core-sidecar-resolution-and-forward-sync-handoff-2026-07-23 1ed2d0facce13ca7b2b104e8641d261e9ce74fe4
fix/core-sync-s1-binary-and-dag-prune-wiring-2026-07-25         c36c3d7db4c750ace3481dd21b8ce9f21f453734
release/build-v2                                                30505d5b8943f62f8d3c1712c574dbc2add0dc5b
release/final                                                   c1513cb0d66adf2fa4c7cf8b96589ef98afabcc3
worktree-agent-a340993b0d230d99c                                79a8de5d09c0b6806f6e719e1d2dda0d2e68126f
worktree-agent-a5ef1d50db1ffb554                                2bcb394f4b95da294902979a832a94ac519973a3
```

## Remote-only branches deleted from origin (2026-07-28)

Not in the local list above (existed only on origin). All verified superseded
by `origin/main`; notably `chore/repin-40204-2026-07-27` pinned an UNDEPLOYED
SBT (`0x3e0c2B1c…43E42`, eth_getCode = 0x) — main's live pin `0x4CE39F89…0cF1`
is correct. The two docs branches were byte-identical to main.

```
origin/chore/repin-40204-2026-07-27                             5ff2657a2d3f4abdabbb55b65abd95d64b90510c
origin/docs/core-finish-plan-2026-07-18                         4f0689e61381d6d8869dd285f2274593de8723e4
origin/docs/node-bringup-linux-mac                              fe8626ae7a8c2f215b6e3ad11ed9fc8abbb051d1
origin/harden/package-alignment-node-spawn                      fde5e08c468a904871c002b754b72420e937a7da
origin/integration/beta-final                                   f997bdaac11b70e0767e252e5b55f09b85ea6f34
```
