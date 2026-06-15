# Benchmark corpus: fictional financial firm "Meridian Capital".
# Facts are deliberately spread across documents so that hard questions
# require assembling 2-3 chunks from DIFFERENT documents, linked only by
# shared entities (INC-2207, desk EU-7, R-77, ...). This implements the
# benchmark criterion agreed in the 2026-06-12 review: difficult,
# multi-statement questions, with single-hop questions as the control.

# Entities used for graph concept nodes. Extraction is exact dictionary
# matching for reproducibility (production would use NER).
ENTITIES = [
    "INC-2207", "TRD-88231", "EU-7", "Policy 4.2.7", "ACME Corp",
    "Atlas OMS", "HeliosDB", "R-77", "Priya Sharma", "Daniel Cho",
]

# (doc_id, title, [chunk texts])
DOCUMENTS = [
    ("incident_2207", "Incident report INC-2207", [
        "INC-2207 (SEV-1): On March 3 at 09:41, the Atlas OMS order router began duplicating "
        "fill messages. The duplication persisted for 11 minutes before the circuit breaker "
        "engaged. Root trigger was a failover event in HeliosDB.",
        "Impact of INC-2207: position reports for desk EU-7 were misstated during the incident "
        "window. Downstream risk dashboards showed stale exposure for the desk.",
        "Resolution of INC-2207: duplicate fills were reversed by 14:00 the same day. A full "
        "re-verification of trades executed in the window was ordered.",
    ]),
    ("blotter_q1", "Q1 trade blotter extracts", [
        "TRD-88231: BUY 500,000 EUR/USD at 1.0844, executed 09:47 March 3, desk EU-7, "
        "trader D. Cho. Status: flagged for review.",
        "TRD-88412: SELL 200,000 GBP/USD at 1.2701, executed March 12, desk LN-2, "
        "trader M. Okafor. Status: settled normally.",
        "Review note: TRD-88231 was flagged because it executed inside the INC-2207 incident "
        "window, while fill confirmations were unreliable.",
    ]),
    ("risk_policy", "Meridian risk policy manual (v4.2)", [
        "Policy 4.2.3: intraday position limits are set per desk by the risk committee and "
        "reviewed quarterly.",
        "Policy 4.2.7: any trade executed during a SEV-1 incident affecting trade capture or "
        "position systems must be independently re-verified within 48 hours, including price "
        "and counterparty confirmation.",
        "Policy 4.2.9: escalation path for limit breaches runs from desk head to CRO within "
        "two hours of detection.",
    ]),
    ("compliance_memo", "Compliance memo, April", [
        "April compliance memo: Policy 4.2.7 was invoked for all trades executed during the "
        "INC-2207 window. Fourteen trades were re-verified.",
        "Of the re-verified trades, one — TRD-88231 — required a price adjustment of 1.2 basis "
        "points after counterparty reconfirmation. All others were confirmed unchanged.",
    ]),
    ("helios_postmortem", "Engineering postmortem: HeliosDB failover", [
        "HeliosDB postmortem: the March 3 failover was caused by clock skew on node hel-03, "
        "which exceeded the lease timeout and forced an unplanned primary election.",
        "Contributing factor: the Atlas OMS retry logic amplified the failover into a duplicate "
        "message storm instead of backing off.",
        "Action items: NTP hardening on all HeliosDB nodes; exponential backoff in Atlas OMS "
        "retry paths; chaos drill scheduled for June.",
    ]),
    ("acme_onboarding", "Client onboarding: ACME Corp", [
        "ACME Corp completed KYC onboarding in January. Primary contact: treasury desk, "
        "settlement via standing SSI instructions.",
        "Routing: all ACME Corp FX orders are routed through desk EU-7 under the standing "
        "execution agreement.",
    ]),
    ("risk_committee_may", "Risk committee minutes, May", [
        "May risk committee: Priya Sharma presented the INC-2207 remediation review. The "
        "committee voted to lower the EU-7 intraday limit to 25M until recon improvements land.",
        "Other business: quarterly limits for LN-2 and NY-4 desks reaffirmed without change.",
    ]),
    ("staffing_note", "Desk staffing note", [
        "Daniel Cho joined desk EU-7 as senior trader in February. He is the designated "
        "approver for EU-7 tickets above 100,000 notional.",
    ]),
    ("system_inventory", "Systems inventory", [
        "Atlas OMS is the order management system handling order routing and fill capture for "
        "all FX desks.",
        "HeliosDB is the position store. The nightly reconciliation job R-77 compares Atlas OMS "
        "fills against HeliosDB positions and reports breaks.",
    ]),
    ("audit_q1", "Q1 internal audit findings", [
        "Q1 audit finding: reconciliation job R-77 runs nightly and therefore missed the "
        "11-minute intraday window of INC-2207 entirely; breaks were only visible after "
        "manual review.",
        "Q1 audit recommendation: replace nightly batch reconciliation with stream-based "
        "reconciliation so intraday incidents are caught while open.",
    ]),
]

# Questions. gold = list of (doc_id, chunk_index) that must ALL be retrieved
# for the answer to be complete. kind: "single" (control) or "multi" (the
# agreed benchmark criterion: multi-statement, cross-document).
QUESTIONS = [
    # --- single-hop controls ---
    {"q": "What does Policy 4.2.7 require for trades executed during a SEV-1 incident?",
     "gold": [("risk_policy", 1)], "kind": "single"},
    {"q": "Who is the senior trader on desk EU-7 and what is his approval threshold?",
     "gold": [("staffing_note", 0)], "kind": "single"},
    {"q": "What caused the HeliosDB failover on March 3?",
     "gold": [("helios_postmortem", 0)], "kind": "single"},
    {"q": "What did the Q1 internal audit recommend about reconciliation?",
     "gold": [("audit_q1", 1)], "kind": "single"},
    {"q": "What does the nightly job R-77 do?",
     "gold": [("system_inventory", 1)], "kind": "single"},
    {"q": "How long did the Atlas OMS fill duplication last during INC-2207?",
     "gold": [("incident_2207", 0)], "kind": "single"},

    # --- hard multi-statement questions (cross-document) ---
    {"q": "Why was trade TRD-88231 flagged for review, and what was the final outcome "
          "after re-verification?",
     "gold": [("blotter_q1", 2), ("compliance_memo", 1)], "kind": "multi"},
    {"q": "Describe the full chain of system failures that led to desk EU-7 positions "
          "being misreported in March.",
     "gold": [("helios_postmortem", 0), ("helios_postmortem", 1), ("incident_2207", 1)],
     "kind": "multi"},
    {"q": "Which policy applied to trades executed during the March incident, and how "
          "many trades did compliance re-verify under it?",
     "gold": [("risk_policy", 1), ("compliance_memo", 0)], "kind": "multi"},
    {"q": "How did the duplicated fills escape detection by the firm's reconciliation "
          "process at the time?",
     "gold": [("audit_q1", 0), ("system_inventory", 1)], "kind": "multi"},
    {"q": "Which client's orders could have been affected by the March order routing "
          "incident, and why?",
     "gold": [("acme_onboarding", 1), ("incident_2207", 1)], "kind": "multi"},
    {"q": "What governance changes followed the March incident for the affected desk, "
          "and who presented the remediation review?",
     "gold": [("risk_committee_may", 0), ("incident_2207", 1)], "kind": "multi"},
]


def chunk_entities(text: str):
    """Exact dictionary entity extraction (deterministic by construction)."""
    found = []
    for e in ENTITIES:
        if e in text or e.replace(" ", "") in text.replace(" ", ""):
            found.append(e)
    # special-case aliases
    if "D. Cho" in text and "Daniel Cho" not in found:
        found.append("Daniel Cho")
    return found
