# Models, Media, and Operations

Use this profile for model catalog changes, source separation, audio
measurement, telemetry, a hosted service, container images, or Kubernetes
resources.

## Authorities

| Authority                                                                                                                                                                                                             | Use in OpenKara                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| [ITU-R BS.1770-5](https://www.itu.int/rec/R-REC-BS.1770-5-202311-I/en)                                                                                                                                                | Loudness, true peak, normalization, limiter behavior, and exported measurement |
| [ISO/IEC 42001:2023](https://www.iso.org/standard/81230.html), [ISO/IEC 23894:2023](https://www.iso.org/standard/77304.html), and [NIST AI RMF 1.0](https://www.nist.gov/itl/ai-risk-management-framework)            | Governance and risk review for model or dataset changes                        |
| Model Cards and Datasheets for Datasets                                                                                                                                                                               | Model capability, limits, evaluation data, provenance, and known risks         |
| [OpenTelemetry](https://opentelemetry.io/docs/specs/) and SRE SLI/SLO practice                                                                                                                                        | A hosted service, telemetry pipeline, or user-critical remote operation        |
| [OCI Image and Distribution Specifications](https://opencontainers.org/) and [Kubernetes API conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md) | Container images, registries, or Kubernetes-native APIs                        |

## Constraints

- BS.1770-5 applies only when the product measures or changes loudness. It
  does not define source-separation quality or playback-clock synchronization.
- A model or catalog change records source, license, checksum, supported
  platform, deterministic evaluation fixture, capability limit, and known user
  effect. Keep model input local unless the user has a clear remote-data choice.
- Do not claim ISO/IEC 42001 certification or AI RMF conformance. These
  authorities guide model and dataset risk review.
- A hosted service or telemetry pipeline defines its SLI, SLO, error budget,
  data purpose, retention, and user control before it collects production data.
- A container or Kubernetes interface uses its matching OCI or API convention
  and records the new product surface in an ADR.

## Required evidence

- Deterministic fixture measurements and tolerances for changed loudness or
  media processing.
- Catalog, provenance, license, checksum, and model-evaluation evidence for a
  changed model.
- Trace, metric, log, SLO, and privacy evidence for a hosted service or
  telemetry change.
