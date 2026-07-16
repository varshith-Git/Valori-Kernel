// Getting-started checklist state. Steps 1-3 are derived live (projects,
// running status, record count); the last two are user actions the server
// can't attribute, so they're tracked as localStorage flags set at the
// moment the action succeeds.

const KEY_SEARCHED = "valori-onboarding:searched";
const KEY_PROOF = "valori-onboarding:proof";
const KEY_DISMISSED = "valori-onboarding:dismissed";

export function markSearched(): void {
  try { localStorage.setItem(KEY_SEARCHED, "1"); } catch {}
}

export function markProofViewed(): void {
  try { localStorage.setItem(KEY_PROOF, "1"); } catch {}
}

export function dismissOnboarding(): void {
  try { localStorage.setItem(KEY_DISMISSED, "1"); } catch {}
}

export function getOnboardingFlags(): { searched: boolean; proof: boolean; dismissed: boolean } {
  try {
    return {
      searched: localStorage.getItem(KEY_SEARCHED) === "1",
      proof: localStorage.getItem(KEY_PROOF) === "1",
      dismissed: localStorage.getItem(KEY_DISMISSED) === "1",
    };
  } catch {
    return { searched: false, proof: false, dismissed: false };
  }
}
