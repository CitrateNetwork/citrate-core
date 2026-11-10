// C1-V V12 / C1-P — the membership price as read from core-membership's price route, or, when it
// was not read (503, error, web preview), the link to the membership site in the system browser.
import { MEMBERSHIP_PRICING_URL, SEE_PRICING, membershipPriceLabel, type MembershipPrice } from "../data/membership";

// C1-P: shows the price only when it was read from core-membership; otherwise the link.
export function PricingLink({ open, price, style }: { open: (url: string) => void; price: MembershipPrice | null; style?: React.CSSProperties }) {
  const served = membershipPriceLabel(price);
  if (served) return <span style={style}>{served}</span>;
  return (
    <a
      href={MEMBERSHIP_PRICING_URL}
      onClick={(e) => {
        e.preventDefault();
        open(MEMBERSHIP_PRICING_URL);
      }}
      style={{ color: "var(--accent-text)", ...style }}
    >
      {SEE_PRICING} ↗
    </a>
  );
}
