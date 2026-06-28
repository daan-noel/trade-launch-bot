/**
 * Per-mode accent token. The frontend split keeps both builds visually nearly
 * identical (same chrome + Tailwind theme); only the **active-nav highlight** and
 * the **logo badge** differ by one accent color — teal for the live
 * build, violet for the lab build. Full class strings (not interpolated)
 * so Tailwind's scanner keeps them.
 */
export type AccentColor = 'teal' | 'violet';

export const accentClasses: Record<AccentColor, { navActive: string; logo: string }> = {
  teal: {
    navActive: 'bg-primary/12 text-primary shadow-[inset_0_1px_0_rgba(19,206,175,0.15)]',
    logo: 'bg-primary/10 text-primary ring-primary/20 group-hover:bg-primary/15',
  },
  violet: {
    navActive: 'bg-violet-500/12 text-violet-300 shadow-[inset_0_1px_0_rgba(139,92,246,0.15)]',
    logo: 'bg-violet-500/10 text-violet-300 ring-violet-500/20 group-hover:bg-violet-500/15',
  },
};
