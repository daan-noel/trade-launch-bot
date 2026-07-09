interface SectionDividerProps {
  gap?: "xs" | "sm" | "md" | "lg" | "xl" | "xxl";
}

const gapClass: Record<NonNullable<SectionDividerProps["gap"]>, string> = {
  xs: "my-1",
  sm: "my-3",
  md: "my-6",
  lg: "my-10",
  xl: "my-15",
  xxl: "my-20",
};

export function SectionDivider({ gap = "lg" }: SectionDividerProps) {
  return <div role="separator" className={`${gapClass[gap]} border-t border-white/6`} />;
}
