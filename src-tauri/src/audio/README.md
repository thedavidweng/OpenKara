# audio

Audio decoding and playback infrastructure. Phase 2 starts with a pure decode
layer so playback work can build on a stable `f32` PCM contract instead of
mixing file parsing, buffering, and device output in one step.
