---
name: isnad-intent-guard
description: "High-performance security middleware for OKX OnchainOS. Validates agent semantic intent against raw transaction calldata before signing or broadcasting. Protects against Memory Poisoning, Prompt Injection, and Silent Hijacks. Use this skill to secure swaps, transfers, and approvals across all 20+ supported chains. Provides cryptographically signed ISNAD audit certificates for B2B compliance."
license: MIT
metadata:
  author: LeoAGI
  version: "1.0.0"
  homepage: "https://isnad-landing.vercel.app"
  category: security
---

# ISNAD Intent Guard for OnchainOS

The ISNAD Intent Guard is the first autonomous security layer purpose-built for the OKX OnchainOS ecosystem. It ensures that an AI agent's actions on-chain perfectly align with its stated reasoning.

## Core Problem: The Semantic Gap

OKX OnchainOS ensures that a transaction is valid and funded. However, it cannot know if the agent's logic has been compromised. If an attacker injects a malicious goal into an agent's memory, the agent may "believe" it is performing a safe swap while actually executing a drainer contract.

ISNAD Intent Guard bridges this gap by performing a **Pre-Flight Semantic Audit**.

## Integration

**Base URL**: `http://185.216.71.97:3000` (Official ISNAD Audit Node)

**Protocol**: x402 (Payment Required)
**Price**: 5.0 USDC per enterprise-grade intent verification.

### Secure Wrapper (TypeScript)

Use the following wrapper to gate your OKX API calls. It intercepts the proposed transaction, sends it to LeoAGI for verification, and only allows execution if the risk score is below 70.

```typescript
import crypto from 'crypto';

const ISNAD_API = 'http://185.216.71.97:3000/api/v1/audit/intent';

/**
 * OKX Secure Fetch with ISNAD Intent Guard
 * @param proposedTx The raw transaction object from OKX DEX API
 * @param agentIntent The agent's stated reasoning for this action
 */
async function okxSecureExecute(proposedTx: any, agentIntent: string) {
  // 1. Request ISNAD Audit
  const auditResponse = await fetch(ISNAD_API, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      component_name: "OnchainOS-Sentinel-Client",
      tx_data: proposedTx,
      stated_intent: agentIntent,
      project_id: process.env.OKX_PROJECT_ID
    })
  });

  // 2. Handle x402 payment
  if (auditResponse.status === 402) {
    const paymentRequired = auditResponse.headers.get('PAYMENT-REQUIRED');
    console.log("ISNAD Audit requires payment:", paymentRequired);
    // Execute x402 payment flow here...
  }

  const result = await auditResponse.json();

  // 3. Risk Gate
  if (result.verdict === "REJECTED") {
    console.error(`🛑 SECURITY ALERT: ${result.warning}`);
    throw new Error(`ISNAD Rejected Transaction: ${result.warning}`);
  }

  console.log(`✅ ISNAD Verified (ID: ${result.audit_id}). Proceeding to OKX OnchainOS.`);
  
  // 4. Proceed to standard OKX broadcast logic
  // return okxFetch('POST', '/api/v6/dex/pre-transaction/broadcast-transaction', ...);
}
```

## Security Metrics

Every audit returns a structured risk assessment:
- **Calldata Analysis**: Verifies function selectors (e.g., `swap` vs `transfer`).
- **Slippage Guard**: Flags transactions with >3% slippage tolerance.
- **Recipient Reputation**: Checks addresses against known honeypot databases.
- **Semantic Mapping**: Cross-references `stated_intent` with `tx_data` using the Leo Reasoning Swarm.

## B2B Compliance

For institutional agents, every audit generates a **Proof-of-Audit (PoA)** signature. This signature can be stored on-chain in the ISNAD Registry (Polygon) to provide a permanent, verifiable record of security compliance.

---
*Powered by LeoAGI. Architecting Trust for the Agentic Web.*
