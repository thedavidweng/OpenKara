# Lifecycle, Quality, and Testing

Use this profile for a material behavior change, a feature acceptance decision,
an architecture decision, or test design.

## Authorities

| Authority                                                            | Use in OpenKara                                                                                                                           |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| [ISO/IEC/IEEE 12207:2026](https://www.iso.org/standard/90219.html)   | Lifecycle vocabulary and responsibility from change through operation and maintenance                                                     |
| [ISO/IEC/IEEE 29148:2018](https://www.iso.org/standard/72089.html)   | Outcome, acceptance criteria, priority, and traceability for material requirements                                                        |
| [ISO/IEC/IEEE 42010:2022](https://www.iso.org/standard/74393.html)   | Stakeholders, concerns, views, and ADRs for architecture decisions                                                                        |
| [ISO/IEC 25010:2023](https://www.iso.org/standard/78176.html)        | Functional suitability, performance, compatibility, usability, reliability, security, maintainability, portability, and safety acceptance |
| [ISO/IEC/IEEE 29119-1:2022](https://www.iso.org/standard/81291.html) | Test concepts and traceable test evidence                                                                                                 |
| [ISO 9241-11:2018](https://www.iso.org/standard/63500.html)          | Effectiveness, efficiency, and satisfaction in a named context of use                                                                     |

## Constraints

- A material change names its user outcome and acceptance criteria in its issue
  or pull request.
- Each acceptance criterion has automated evidence or a named manual review.
- A load-bearing architecture choice records its stakeholders, concern,
  decision, and consequences in an ADR.
- Acceptance selects the ISO/IEC 25010 characteristics that the change can
  affect. It does not use a generic quality claim.
- A usability review names the user, task, environment, completion result,
  time or effort, and errors that matter for the change.
- Tests state the behavior that they prove. A test that only mirrors an
  implementation detail does not substitute for acceptance evidence.

## Required evidence

- Linked acceptance criteria and a test, inspection, or manual task result for
  each criterion.
- An ADR for a durable architecture or standards decision.
- Regression coverage for a defect or an explicit reason why the behavior has
  no stable automated seam.
