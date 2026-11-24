<!-- TEMPLATE — Business / Policy Reference (skills/<skill>/references/<module>.md). Replace {placeholders}; use only the ## headings below. The three-column Hot command table is allowed only here. -->
# {Module name}

<!-- Optional. Include only when it carries an otherwise missing boundary. -->
## Module Description
{Applies when, business boundary, and route-out condition.}

## State Routing
<!-- Optional opening constraint: state which trusted upstream state may enter this module. Do not use this for a security policy. -->
| Input or state | Reference |
| --- | --- |
| ... | [module-reference](module-reference.md) |

## Intent Routing
<!-- Optional: only atomic direct-command intents. -->
| User intent | Reference | Hot command |
| --- | --- | --- |
| ... | — | `onchainos ...` |

## Action Routing
<!-- Optional. -->
| Action ID | Reference |
| --- | --- |
| ... | [next-module-reference](next-module-reference.md) |

## Flow
### Collect Parameters
### Execute
### Handle Result

## Security
### Confirmation
### Risk and Stop Conditions

## Output Rules
<!-- Optional: what the agent must show, omit, or say. -->
### Render Rules
<!-- Optional: module-specific presentation that cannot live in Root. -->

## Chain Name Support
<!-- Optional. -->

## Data Freshness
<!-- Optional. -->

## Troubleshooting
<!-- Optional. -->

## CLI Reference
- [module-cli-reference.md](module-cli-reference.md)
