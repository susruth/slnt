# Governance

SLNT currently uses maintainer-led governance.

## Decision model

- The sRFC is the source of truth for protocol behavior.
- Maintainers make day-to-day repository decisions.
- Protocol changes should be discussed publicly and should reach rough
  consensus before implementation.
- Security fixes may be handled privately until disclosure is safe.

## Protocol changes

Anything that changes interoperability is a protocol change, including:

- meta-address encoding,
- domain-separation tags,
- key derivation,
- announcement or registry layouts,
- `scheme_id` or version semantics,
- sender/recipient scan or sweep requirements.

Start these as a GitHub Discussion. Once the shape is accepted, update the
sRFC and then implement the code changes.

## Implementation changes

Implementation changes that keep the repo aligned with the sRFC can be normal
pull requests. Maintainers may merge small fixes directly after review.

## Maintainers

Current maintainer:

- susruth (<susruth@susruth.com>)

New maintainers may be added after sustained, high-quality contribution and
maintainer agreement.

## Deployments

Canonical deployments are security-sensitive. Program IDs, upgrade authority,
immutability, and migration plans should be documented before a deployment is
treated as canonical.
