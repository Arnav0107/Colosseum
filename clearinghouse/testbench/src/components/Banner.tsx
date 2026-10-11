/** Always visible: this page is a localnet tool, not a product. */
export const BANNER_TEXT = "LOCALNET · MOCK ORACLE · UNVERIFIED VENUES";

export function Banner() {
  return (
    <div className="banner" role="note" aria-label="Environment warning">
      <strong>{BANNER_TEXT}</strong>
      <span className="banner-detail">
        Dev keys, admin-set prices, venues that self-report. Nothing here is a real credit decision.
      </span>
    </div>
  );
}
