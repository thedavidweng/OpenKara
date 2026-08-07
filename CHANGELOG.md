# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.13.0](https://github.com/thedavidweng/OpenKara/compare/v0.12.1...v0.13.0) (2026-08-07)


### Features

* **ui:** add copy debug info action on error toasts ([55bf6f3](https://github.com/thedavidweng/OpenKara/commit/55bf6f308e52c5d5e5528cdd5dd98efe70e2a25c))


### Bug Fixes

* address PR review findings still valid in tree ([7c57231](https://github.com/thedavidweng/OpenKara/commit/7c57231603a5ad7d90c8c066b40c1c00582d1809))
* apply CodeRabbit auto-fixes ([030a2fd](https://github.com/thedavidweng/OpenKara/commit/030a2fdb1f95f1bfa5c2f13d9168b2489a976a2d))
* apply CodeRabbit auto-fixes ([862fc10](https://github.com/thedavidweng/OpenKara/commit/862fc10aca9f71ae4632facee5cc53569c30675f))
* **ci:** align pnpm action version with packageManager 11.20.0 ([f6d3037](https://github.com/thedavidweng/OpenKara/commit/f6d303751fbbf81daee8571a1f8a4edad96f542f))
* **ci:** align pnpm pins with packageManager 11.20.0 ([64046b4](https://github.com/thedavidweng/OpenKara/commit/64046b402acaf4e4bb0d4790f747fb69e2c266d4))
* **ci:** re-create draft releases bound to the real tag ([d915a74](https://github.com/thedavidweng/OpenKara/commit/d915a74905867823c43ab986d281b580191284cd))
* pass initial value to useRef in ToastContainer ([e28b450](https://github.com/thedavidweng/OpenKara/commit/e28b4508fb082fc3d7492e42cfdca27cd9be2ca6))
* **runtime:** harden Windows ORT load and sticky app language ([a34b961](https://github.com/thedavidweng/OpenKara/commit/a34b961d9b039a57a937ef1300f18f96ca664d84))
* **runtime:** Windows ORT load, sticky language, agent docs index ([30c4c76](https://github.com/thedavidweng/OpenKara/commit/30c4c76f555c29baae4e3527095b393e91479e88))

## [0.12.1](https://github.com/thedavidweng/OpenKara/compare/v0.12.0...v0.12.1) (2026-08-04)


### Bug Fixes

* address release review findings ([f49323c](https://github.com/thedavidweng/OpenKara/commit/f49323cd3699f1f51cb1218fb0da96e086c3e44e))
* **ci:** resolve nightly-hardening.yml Windows and installed-app failures ([ee00b0e](https://github.com/thedavidweng/OpenKara/commit/ee00b0e7ef5203ac5005941c8c93116cecafc33a))
* **ci:** scope release workflow permissions ([6a7017d](https://github.com/thedavidweng/OpenKara/commit/6a7017d51500ba445f0614852827814f9859787b))
* **ci:** scope release workflow permissions ([624d0ba](https://github.com/thedavidweng/OpenKara/commit/624d0ba34c81d03e5d568e7f546095bfa451c34d))
* close issue 303 release gates and prune remote playback ([96dcd20](https://github.com/thedavidweng/OpenKara/commit/96dcd20e5bb148cb35bbd4eb5a3f0f8e097ed951))
* close release hardening gaps ([4e6f878](https://github.com/thedavidweng/OpenKara/commit/4e6f8786b646527e9ebf44b32b5a5e7209bfc0ee))
* guard stem attachment against stale requests ([f802618](https://github.com/thedavidweng/OpenKara/commit/f802618ef3639326aba86663b32525317ea8b142))
* include e2e dependencies in knip ([de31a6b](https://github.com/thedavidweng/OpenKara/commit/de31a6bd2c61af2ca4dd5bb1fe01b168c9e559c3))
* preserve stale playback cancellation ([cd1d65b](https://github.com/thedavidweng/OpenKara/commit/cd1d65bac9f4b5e500fc63fe3e5b4eeaaa5af8cf))
* **runtime:** terminate bootstrap after completed download ([316f1a6](https://github.com/thedavidweng/OpenKara/commit/316f1a62156511d492674755fe31aa077521f2c3)), closes [#284](https://github.com/thedavidweng/OpenKara/issues/284)
* synchronize playback request invalidation ([652a4ae](https://github.com/thedavidweng/OpenKara/commit/652a4ae0b36d0b0d907a4ee348d55b1b9edd4cb4))
* tighten desktop and stem loading checks ([e404e97](https://github.com/thedavidweng/OpenKara/commit/e404e97ebf57f7da32a5c812d48db1a56bd8e907))

## [0.12.0](https://github.com/thedavidweng/OpenKara/compare/v0.11.0...v0.12.0) (2026-08-02)


### Features

* **release:** auto-publish after assets, then submit distribution PRs ([#309](https://github.com/thedavidweng/OpenKara/issues/309)) ([83a9481](https://github.com/thedavidweng/OpenKara/commit/83a9481f5955250962c8df7c23f9b7a222449519))


### Bug Fixes

* **ci:** stop silent release-please tag failures after merge ([#308](https://github.com/thedavidweng/OpenKara/issues/308)) ([72b0fc8](https://github.com/thedavidweng/OpenKara/commit/72b0fc807d1d82d158293c51f855b05630d22c77))
* **i18n:** route user-facing copy through translations ([#312](https://github.com/thedavidweng/OpenKara/issues/312)) ([c0894d7](https://github.com/thedavidweng/OpenKara/commit/c0894d73c71570cc39d22e5ff2753ef9830b1bfd))
* **release:** apply installation template to draft releases ([#313](https://github.com/thedavidweng/OpenKara/issues/313)) ([ee00738](https://github.com/thedavidweng/OpenKara/commit/ee00738b1af2c7aa0e43c9cdccb02106c57d453b))

## [0.11.0](https://github.com/thedavidweng/OpenKara/compare/v0.10.0...v0.11.0) (2026-08-01)

### Features

- **automation:** add canonical report schema and contract test for release gate ([6f12a2e](https://github.com/thedavidweng/OpenKara/commit/6f12a2e60b80522faffa48afd0256f3d48b2f15a))
- **automation:** add openkara_automation_driver and canonical report builder ([8156c88](https://github.com/thedavidweng/OpenKara/commit/8156c885940160d3bd7420ea328539e8be759252))
- **automation:** validate audio outputs, runtime/model digests, and [#284](https://github.com/thedavidweng/OpenKara/issues/284) assertions ([3b52513](https://github.com/thedavidweng/OpenKara/commit/3b525131d714c347c0ad7cf2b05e677ad1b4b09d))

### Bug Fixes

- **automation:** 1.0 desktop and accessibility release gate ([#305](https://github.com/thedavidweng/OpenKara/issues/305)) ([a072087](https://github.com/thedavidweng/OpenKara/commit/a0720879f2777a245f2341fa0435acf595e2d23c))

## [0.10.0](https://github.com/thedavidweng/OpenKara/compare/v0.9.1...v0.10.0) (2026-07-29)

### Features

- **audio:** make the equalizer readable with named bands, a dB scale, and presets ([#237](https://github.com/thedavidweng/OpenKara/issues/237)) ([#243](https://github.com/thedavidweng/OpenKara/issues/243)) ([46da356](https://github.com/thedavidweng/OpenKara/commit/46da35617369341670ba6efaec5321afb10b9b2d))
- **catalog:** advance the embedded snapshot to generation 9 (reduced runtimes) ([#224](https://github.com/thedavidweng/OpenKara/issues/224)) ([86218ae](https://github.com/thedavidweng/OpenKara/commit/86218aeef14bdbd5f7d33c267f03554ac7f9e418))
- **diagnostics:** show where the model file belongs ([#282](https://github.com/thedavidweng/OpenKara/issues/282)) ([62ace29](https://github.com/thedavidweng/OpenKara/commit/62ace297b63f3f043db79bf181375b1ba54c7e17))
- **i18n:** expand UI language coverage to 17 languages ([#227](https://github.com/thedavidweng/OpenKara/issues/227)) ([#253](https://github.com/thedavidweng/OpenKara/issues/253)) ([02c067a](https://github.com/thedavidweng/OpenKara/commit/02c067a066b2323ff52f7fb653de5b9da0d2a509))
- **lyrics:** add fullscreen lyric alignment toggle and fix audience truncation ([dd06e48](https://github.com/thedavidweng/OpenKara/commit/dd06e48f15710101a634869b95c3e7698bb19dda))
- **lyrics:** fullscreen alignment toggle and audience truncation fixes ([b869144](https://github.com/thedavidweng/OpenKara/commit/b86914415b277cdc4a18f494f0aefb228f245271))
- **model:** consume the openkara-models stable catalog with installed identity ([#179](https://github.com/thedavidweng/OpenKara/issues/179)) ([037a6e6](https://github.com/thedavidweng/OpenKara/commit/037a6e663807a4ab16ab662942c52a37785653d1))
- **observability:** file logging, cross-platform About, and useful debug export ([#231](https://github.com/thedavidweng/OpenKara/issues/231)) ([687da3a](https://github.com/thedavidweng/OpenKara/commit/687da3afef4bf6a79b64031c824b0a03f0b07429))
- **remote:** give a Pre-Publish Conflict a way out, and clear the dead code behind it ([#283](https://github.com/thedavidweng/OpenKara/issues/283)) ([15ef164](https://github.com/thedavidweng/OpenKara/commit/15ef164c37c40145b035fc7a79979f699f221049))
- **runtime:** catalog-driven ONNX Runtime lifecycle with staged activation ([#184](https://github.com/thedavidweng/OpenKara/issues/184)) ([7e36fec](https://github.com/thedavidweng/OpenKara/commit/7e36fec04b87491f5ff3952fa8a68236bc865469))
- **separation:** cancel single-song runs and surface cache hits ([#180](https://github.com/thedavidweng/OpenKara/issues/180), [#181](https://github.com/thedavidweng/OpenKara/issues/181)) ([#183](https://github.com/thedavidweng/OpenKara/issues/183)) ([f6b4e73](https://github.com/thedavidweng/OpenKara/commit/f6b4e73be579a411fc68e6f78d571c2ac2c88da5))
- **separation:** default new separations to four stems ([#185](https://github.com/thedavidweng/OpenKara/issues/185)) ([bd90d76](https://github.com/thedavidweng/OpenKara/commit/bd90d76c79218424c217038e870d397e0cec90a6))
- **separation:** notify natively when a separation finishes unfocused ([#268](https://github.com/thedavidweng/OpenKara/issues/268)) ([1672daf](https://github.com/thedavidweng/OpenKara/commit/1672daff2b09c2a6cab798675b83d5fe29393841))
- **separator:** native STFT/ISTFT for the spectral contract v1 ([#172](https://github.com/thedavidweng/OpenKara/issues/172)) ([#186](https://github.com/thedavidweng/OpenKara/issues/186)) ([ec4f86f](https://github.com/thedavidweng/OpenKara/commit/ec4f86fc32bb150895d4ddf99024d3ffe7a09f2b))
- **separator:** spectral-core stable switch — generation 8 + waveform path deletion ([#172](https://github.com/thedavidweng/OpenKara/issues/172) PR 5) ([#197](https://github.com/thedavidweng/OpenKara/issues/197)) ([b416cdf](https://github.com/thedavidweng/OpenKara/commit/b416cdf7917dc708bf933e095c8706c5025219cb))
- **separator:** typed spectral-core session path ([#172](https://github.com/thedavidweng/OpenKara/issues/172) PR 2) ([#192](https://github.com/thedavidweng/OpenKara/issues/192)) ([0a785a5](https://github.com/thedavidweng/OpenKara/commit/0a785a5ce2da42ecaad1ffd562789ade99067994))
- **settings:** add hide-upgrade-all option and show song count in global progress ([1d02592](https://github.com/thedavidweng/OpenKara/commit/1d02592a3f753c20d6d65becd8bb52597a8278df))
- **shell:** adopt the official updater plugin for in-app updates ([#255](https://github.com/thedavidweng/OpenKara/issues/255)) ([#259](https://github.com/thedavidweng/OpenKara/issues/259)) ([3a60fde](https://github.com/thedavidweng/OpenKara/commit/3a60fde8ca107d2838939fcc5a94f60171c4ae0d))
- **shell:** enforce a single app instance and open URLs through the opener plugin ([#258](https://github.com/thedavidweng/OpenKara/issues/258)) ([18ea7ac](https://github.com/thedavidweng/OpenKara/commit/18ea7acc3396bcf30dfce005a81dd9d935906c04))
- **shell:** persist main window geometry across launches ([#267](https://github.com/thedavidweng/OpenKara/issues/267)) ([b31f375](https://github.com/thedavidweng/OpenKara/commit/b31f375b860c3ab81a449884175c4b258da43a92)), closes [#263](https://github.com/thedavidweng/OpenKara/issues/263)
- **standards:** adopt product quality baselines ([#295](https://github.com/thedavidweng/OpenKara/issues/295)) ([25ffb95](https://github.com/thedavidweng/OpenKara/commit/25ffb9502c345532fb53f0a2578a8425ea718a87))
- **test:** automate release acceptance gates ([#294](https://github.com/thedavidweng/OpenKara/issues/294)) ([87c23a8](https://github.com/thedavidweng/OpenKara/commit/87c23a80503dd9c14e8c4186436e0d237d6cfca2))

### Bug Fixes

- **audience:** let fullscreen lyrics own the whole monitor and hide controls on pointer idle ([#234](https://github.com/thedavidweng/OpenKara/issues/234)) ([#241](https://github.com/thedavidweng/OpenKara/issues/241)) ([7a1d9df](https://github.com/thedavidweng/OpenKara/commit/7a1d9df4082055fe7142d0d12a3d9c06e94594b4))
- **audio:** keep the accompaniment master and its sub-stems in lockstep ([#235](https://github.com/thedavidweng/OpenKara/issues/235)) ([#240](https://github.com/thedavidweng/OpenKara/issues/240)) ([e75a5e9](https://github.com/thedavidweng/OpenKara/commit/e75a5e9564ee666ddb1ecca0f0b684be20612ad1))
- **audio:** recover playback when the output device disconnects ([#273](https://github.com/thedavidweng/OpenKara/issues/273)) ([7a9d0a5](https://github.com/thedavidweng/OpenKara/commit/7a9d0a594994b614d3cee930a25d5075a0a99214)), closes [#250](https://github.com/thedavidweng/OpenKara/issues/250)
- **bootstrap:** make the model-download failed banner recoverable ([#217](https://github.com/thedavidweng/OpenKara/issues/217)) ([#220](https://github.com/thedavidweng/OpenKara/issues/220)) ([f66947d](https://github.com/thedavidweng/OpenKara/commit/f66947daafcbb8a4f60b557452632723879d011a))
- **bootstrap:** resume artifact downloads instead of restarting them ([#281](https://github.com/thedavidweng/OpenKara/issues/281)) ([dc83f4f](https://github.com/thedavidweng/OpenKara/commit/dc83f4f563ae823cc6123414d9d551a10a9cc50f))
- **cache:** preserve songs.language through the legacy schema rebuild ([#221](https://github.com/thedavidweng/OpenKara/issues/221)) ([c9f4e0a](https://github.com/thedavidweng/OpenKara/commit/c9f4e0af17a96eacfcc3fe2fd4f839fbe11cc84d)), closes [#219](https://github.com/thedavidweng/OpenKara/issues/219)
- **ci:** portable model digest verification in spectral-candidate ([#196](https://github.com/thedavidweng/OpenKara/issues/196)) ([f7afd8a](https://github.com/thedavidweng/OpenKara/commit/f7afd8ae71f13a26e54b8494491110866329a791))
- **ci:** set GH_REPO in generate-checksums job ([2a08ba7](https://github.com/thedavidweng/OpenKara/commit/2a08ba7e5bd866fbee535105bc18df389032cce5))
- **ci:** stabilize runtime and lyrics validation ([#293](https://github.com/thedavidweng/OpenKara/issues/293)) ([b8ff5e2](https://github.com/thedavidweng/OpenKara/commit/b8ff5e248e11b91dd7e93357ef1240adc34cf884))
- **clippy:** resolve lint errors and workflow shellcheck warnings ([8cd4239](https://github.com/thedavidweng/OpenKara/commit/8cd42390daf28f87d69f3d7b875b5d1d210022e9))
- **config:** atomic write + corruption recovery to end the boot brick ([#208](https://github.com/thedavidweng/OpenKara/issues/208)) ([#211](https://github.com/thedavidweng/OpenKara/issues/211)) ([cd520d8](https://github.com/thedavidweng/OpenKara/commit/cd520d8d79b9c8fdaba3cb1a5d7c3ac4940d1182))
- **e2e:** deflake webkit lyrics-follow and muted-stem tests, surface flaky counts in CI ([#218](https://github.com/thedavidweng/OpenKara/issues/218)) ([#222](https://github.com/thedavidweng/OpenKara/issues/222)) ([102df50](https://github.com/thedavidweng/OpenKara/commit/102df502c41bc3685c4c8381a068044888d8f385))
- **eq:** remove auto-preamp cancellation so band gains act as absolute boosts ([a90aa20](https://github.com/thedavidweng/OpenKara/commit/a90aa20190999bf724f8beb0ede807b21371ac31))
- **hooks:** skip the pre-push gates when a push only deletes remote refs ([#257](https://github.com/thedavidweng/OpenKara/issues/257)) ([fa5cadc](https://github.com/thedavidweng/OpenKara/commit/fa5cadc525b0a153cf45680b816efe2c718a0a82))
- **hooks:** stop passing oxfmt-ignored src-tauri files to the pre-commit hook ([#193](https://github.com/thedavidweng/OpenKara/issues/193)) ([b2ec4f3](https://github.com/thedavidweng/OpenKara/commit/b2ec4f3d20635800d22a8afe7c47bac72739c8d3))
- **i18n:** translate remote-library flow strings for zh-CN ([#209](https://github.com/thedavidweng/OpenKara/issues/209)) ([#212](https://github.com/thedavidweng/OpenKara/issues/212)) ([1881b6c](https://github.com/thedavidweng/OpenKara/commit/1881b6c05a975909f8670e31b4bf21a33d404fe6))
- **library:** atomic media import and net downgrade savings ([#206](https://github.com/thedavidweng/OpenKara/issues/206), [#207](https://github.com/thedavidweng/OpenKara/issues/207)) ([#213](https://github.com/thedavidweng/OpenKara/issues/213)) ([a09a3f0](https://github.com/thedavidweng/OpenKara/commit/a09a3f04b0563c9803b2777264007429a1ebc65e))
- **library:** name the destination on the remote-repository connect button ([#238](https://github.com/thedavidweng/OpenKara/issues/238)) ([#244](https://github.com/thedavidweng/OpenKara/issues/244)) ([c8d36ba](https://github.com/thedavidweng/OpenKara/commit/c8d36ba04a256c4bc3f24303742fb43b5a7ca39d))
- **library:** record the two-stem downgrade before deleting what it replaces ([#274](https://github.com/thedavidweng/OpenKara/issues/274)) ([ab0f004](https://github.com/thedavidweng/OpenKara/commit/ab0f00452273043c2c76c2cf4b02faa861f1fe30)), closes [#251](https://github.com/thedavidweng/OpenKara/issues/251)
- **lyrics:** add a Reset button to the lyrics offset control ([#233](https://github.com/thedavidweng/OpenKara/issues/233)) ([#247](https://github.com/thedavidweng/OpenKara/issues/247)) ([772489d](https://github.com/thedavidweng/OpenKara/commit/772489d764b0810836fb84660e2d05d45270f8c4))
- **lyrics:** eliminate alignment-toggle flash by stabilizing LyricLine children ([6771557](https://github.com/thedavidweng/OpenKara/commit/6771557450eb1fc544cb2b3ec31b2d5c5586aff8))
- **lyrics:** keep interleaved romaji out of the lyric list ([#232](https://github.com/thedavidweng/OpenKara/issues/232)) ([#246](https://github.com/thedavidweng/OpenKara/issues/246)) ([8bdb68b](https://github.com/thedavidweng/OpenKara/commit/8bdb68b162675f46f02efb20ea5cf2ac4aed118a))
- **lyrics:** make seekable lyric button fill width to restore centered text ([a96c343](https://github.com/thedavidweng/OpenKara/commit/a96c3436f7b38fa0e9617443063912abe051d37e))
- **lyrics:** pin the romanization script from SongLanguage and bump lyric-romanizer to 0.3.0 ([#248](https://github.com/thedavidweng/OpenKara/issues/248)) ([#252](https://github.com/thedavidweng/OpenKara/issues/252)) ([d659b88](https://github.com/thedavidweng/OpenKara/commit/d659b8818a084f2d7251a6d9a833f21bec2ce41a))
- **lyrics:** stabilize scroll follow across ambience, layout, and resize ([#214](https://github.com/thedavidweng/OpenKara/issues/214)) ([6dbefb1](https://github.com/thedavidweng/OpenKara/commit/6dbefb1550b015db6baba04f878bb8f865b723a9))
- **lyrics:** stop auto-upgrade from overwriting user-authored lyrics ([#203](https://github.com/thedavidweng/OpenKara/issues/203)) ([#215](https://github.com/thedavidweng/OpenKara/issues/215)) ([525a2c7](https://github.com/thedavidweng/OpenKara/commit/525a2c78ff44e9bfe36926f400f50ce1b345e54a))
- **release:** gate tag/version consistency and refresh install docs for 1.0 ([#229](https://github.com/thedavidweng/OpenKara/issues/229)) ([0ad817c](https://github.com/thedavidweng/OpenKara/commit/0ad817ce96cb0ed947f0c203f3dcfbd75cb22dec))
- **remote:** bound streaming range fetches and stream downloads to disk ([#204](https://github.com/thedavidweng/OpenKara/issues/204), [#205](https://github.com/thedavidweng/OpenKara/issues/205)) ([#216](https://github.com/thedavidweng/OpenKara/issues/216)) ([f1afa6e](https://github.com/thedavidweng/OpenKara/commit/f1afa6edd2cc7f0d59a96e9f20911d5a1191c59b))
- restore playback clock and unify managed runtime ([#178](https://github.com/thedavidweng/OpenKara/issues/178)) ([030053c](https://github.com/thedavidweng/OpenKara/commit/030053cba7d2f1bc6c6b62cad3f5c8718a871668))
- **runtime:** make the ONNX Runtime install succeed and report one truth ([#236](https://github.com/thedavidweng/OpenKara/issues/236)) ([#245](https://github.com/thedavidweng/OpenKara/issues/245)) ([8ec7cd5](https://github.com/thedavidweng/OpenKara/commit/8ec7cd5bf6d2e357c720dd1ad83ea39bfdfc837a))
- **runtime:** surface the first-install ONNX Runtime download as a named task ([#226](https://github.com/thedavidweng/OpenKara/issues/226)) ([#230](https://github.com/thedavidweng/OpenKara/issues/230)) ([40f2d5b](https://github.com/thedavidweng/OpenKara/commit/40f2d5b6301cf3681577c58e348b4f738013432a))
- **setup:** make the first-run flow honest about what it is asking ([#272](https://github.com/thedavidweng/OpenKara/issues/272)) ([a5bda41](https://github.com/thedavidweng/OpenKara/commit/a5bda412090c0336ee65e3d5ece8f297a8262377))
- **shell:** isolate ObjC exceptions in the macOS window shell bridge ([#265](https://github.com/thedavidweng/OpenKara/issues/265)) ([3b2b4b0](https://github.com/thedavidweng/OpenKara/commit/3b2b4b0df84379fe87586e1cb92b8e338a409add)), closes [#261](https://github.com/thedavidweng/OpenKara/issues/261)
- **shell:** reveal the main window when the frontend handshake never arrives ([#266](https://github.com/thedavidweng/OpenKara/issues/266)) ([dffb487](https://github.com/thedavidweng/OpenKara/commit/dffb487334481d1ee115da5078163a5eb694c33a)), closes [#260](https://github.com/thedavidweng/OpenKara/issues/260)
- **shell:** stop the launch flash and reveal the window only once it can paint ([#239](https://github.com/thedavidweng/OpenKara/issues/239)) ([#242](https://github.com/thedavidweng/OpenKara/issues/242)) ([fc280cd](https://github.com/thedavidweng/OpenKara/commit/fc280cd04d89c4a38c60da5bf607195e16352531))
- **standards:** apply product quality baselines across the codebase ([#296](https://github.com/thedavidweng/OpenKara/issues/296)) ([1977cc6](https://github.com/thedavidweng/OpenKara/commit/1977cc61e2f94748c2e4327f4322c80bf6f94fd2))
- **ui:** stop painting the same separation progress twice ([#285](https://github.com/thedavidweng/OpenKara/issues/285)) ([11f95d4](https://github.com/thedavidweng/OpenKara/commit/11f95d4902931edd1b1ddeacd540d6e948c0b1af))

### Performance Improvements

- **config:** measured per-target execution-provider defaults ([#170](https://github.com/thedavidweng/OpenKara/issues/170)) ([#199](https://github.com/thedavidweng/OpenKara/issues/199)) ([6ecbdfa](https://github.com/thedavidweng/OpenKara/commit/6ecbdfae54fa8d789e6bbdd1ac51cef3034f0465))
- **library:** serve cover-art thumbnails through the asset protocol ([#271](https://github.com/thedavidweng/OpenKara/issues/271)) ([7cd7c73](https://github.com/thedavidweng/OpenKara/commit/7cd7c73c2bda580c90b6c00dffe591d5b6a7244e))
- **runtime:** size intra-op threads to performance cores on Apple Silicon ([#170](https://github.com/thedavidweng/OpenKara/issues/170)) ([#256](https://github.com/thedavidweng/OpenKara/issues/256)) ([316ef50](https://github.com/thedavidweng/OpenKara/commit/316ef50ae0bd0143a1456f140b10118076fa3efc))
- **separator:** bounded-memory streaming separation with OLA ring buffers ([#176](https://github.com/thedavidweng/OpenKara/issues/176)) ([c3eebe1](https://github.com/thedavidweng/OpenKara/commit/c3eebe1338e3ac859d7737d07b7abc916266b9ad))
- **separator:** fixed per-chunk working memory for the spectral session ([#172](https://github.com/thedavidweng/OpenKara/issues/172) PR 3) ([#194](https://github.com/thedavidweng/OpenKara/issues/194)) ([9f742cc](https://github.com/thedavidweng/OpenKara/commit/9f742cc4c9439e42496b0e9b5f6700e14550fce0))

## [0.9.1] - 2026-07-22

### 🐛 Bug Fixes

- **scripts**: Copy .nupkg to .zip stub for Expand-Archive on Windows
