# separator

Stem separation backend built around the embedded Demucs ONNX model.

Current Phase 3 coverage:
- load the embedded ONNX model from `src-tauri/models/`
- preprocess decoded stereo PCM into the model's fixed input window
- run ORT inference, including zero-filled auxiliary tensors required by the model
- extract the final stem output and write named WAV files for `drums`, `bass`,
  `other`, and `vocals`
- mix `drums + bass + other` into a normalized accompaniment WAV

Later Phase 3 work still needs to add caching, progress events, and background
job execution.
