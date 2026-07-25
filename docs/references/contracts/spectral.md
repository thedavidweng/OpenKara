# Spectral transform contract

OpenKara implements the published spectral tensor contract
`openkara.spectral-contract/v1` natively (FFT), rather than through the dense
conv1d/conv_transpose1d DFT matrices baked into the shipped HTDemucs graphs
(~134 MB of constants per model). Implementing the transforms natively lets the
spectral-core models of `openkara-models#23` drop those constants.

The native implementation lives in `src-tauri/src/separator/spectral.rs`
(`SpectralPlans::spec` / `SpectralPlans::ispec`, plus the `magnitude`
neural-core view helper) and is validated against the pinned golden vectors in
`src-tauri/tests/fixtures/spectral/` by `src-tauri/tests/phase7_spectral.rs`.

## Constants

| Name             | Value         | Meaning                                                     |
| ---------------- | ------------- | ----------------------------------------------------------- |
| `sample_rate`    | 44100         | Hz                                                          |
| `channels`       | 2             | stereo                                                      |
| `n_fft`          | 4096          | FFT size                                                    |
| `hop`            | 1024          | hop length (`n_fft / 4`)                                    |
| `window`         | periodic Hann | `sin²(π·n/4096)` (torch default; NOT the symmetric variant) |
| `contract_freqs` | 2048          | one-sided bins carried (Nyquist bin dropped from 2049)      |
| `outer_pad`      | 1536          | Demucs outer reflect padding (`hop/2·3`)                    |
| `envelope_clamp` | 1e-8          | ISTFT overlap-add envelope floor                            |
| `norm`           | 1/√4096       | applied in BOTH directions (`normalized=True`)              |

The forward tensor layout is `[B, C, {real, imag}, freq, frame]`, contiguous;
`le = ceil(samples / hop)` frames. The imaginary convention is `e^{−i2πkn/N}`
(the imaginary part uses `−sin`, matching `torch.stft`), which is exactly what a
real→complex FFT produces. The neural-core `magnitude` view is a pure reshape
`[B, C, 2, F, T] → [B, C·2, F, T]`, channel-major `[L_re, L_im, R_re, R_im]`.

## Validation

The transforms are ported operation-by-operation from the float64 numpy
reference (`spectral_reference.py`) and validated against `spectral-golden-v1`
at every intermediate stage (`spec`, `magnitude`, and the identity `ispec`
round trip) for the silence, impulse, tone, band-limited-noise, and sweep
fixtures. Each stored golden array is SHA-256-verified (against
`spectral-golden-v1.json` → `release_asset_sha256`) before it is trusted.

- **Contract gate:** an fp32 implementation must stay within `1e-3` max-abs of
  the reference per stage (`1e-6` if it computes in f64).
- **Achieved:** the OpenKara implementation computes internally in f64 and casts
  to f32 only at the boundary, landing near f32 round-off (well under `1e-3`).
  The test prints the achieved max-abs per fixture per stage.

## Reconstruction validity (segment stitching)

The Nyquist bin (22050 Hz) is discarded by the forward transform and
reconstructed as zero, so only band-limited content round-trips exactly;
broadband content leaks ~−80 dB of Hann-windowed energy into that bin, bounding
broadband round-trip error near `1e-4` max-abs. Independently, the first and
last `n_fft` (4096) samples of a reconstructed window lose overlap-add
contributions from the cropped frames (interior error ~`1e-10`, transition band
up to ~`3e-6`). Any application that stitches segments MUST overlap at least
`4096` samples per side and cross-fade so only interior samples are used. This
matches the shipped waveform models identically. (No OLA integration ships in
this contract layer; it is provided by the streaming separation path.)

## Versioning and status

Any semantic change (constants, layouts, padding, normalization ownership,
tolerances) requires a `/v2` contract and new golden vectors. Session and cache
identities that depend on the transform semantics must carry the contract
version string (`SPECTRAL_CONTRACT_VERSION`).

The pure DSP layer (issue #172 PR 1) carries no production model path. The
typed spectral session path (issue #172 PR 2) lives in
`src-tauri/src/separator/spectral_session.rs`:

- Dispatch is decided ONLY by the model's embedded metadata
  (`openkara.tensor_interface = "spectral-core"` +
  `openkara.spectral_contract`); output-rank and filename heuristics are
  forbidden. Unknown interfaces and unsupported contract versions fail at
  model load, before any ORT session is created.
- The tensor interface (`spectral`/`mix` → `spectral_out`/`time_out`, fixed
  contract shapes) is verified at load time
  (`spectral_session::verify_spectral_interface`).
- Stems compose as `stems[s] = ispec(spectral_out[:, s]) + time_out[:, s]`.
  FourStem composes each source; TwoStem composes vocals directly and
  pre-mixes the accompaniment in the spectral domain (contract linearity —
  one inverse transform instead of three).
- The session cache key of a spectral-core model carries the contract
  version (`model::session_cache_key`), so a `/v2` contract can never reuse
  a `/v1` session.

Catalog entries for spectral-core artifacts declare
`model.tensor_interface = "spectral-core"`; the embedded catalog gate accepts
exactly the `waveform` and `spectral-core` interfaces.
