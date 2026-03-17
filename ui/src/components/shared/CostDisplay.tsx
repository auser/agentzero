interface CostDisplayProps {
  microdollars?: number
  className?: string
}

export function CostDisplay({ microdollars, className }: CostDisplayProps) {
  if (microdollars === undefined || microdollars === null) {
    return <span className={className}>—</span>
  }
  const dollars = microdollars / 1_000_000
  const formatted = dollars < 0.01
    ? `$${dollars.toFixed(6)}`
    : `$${dollars.toFixed(4)}`
  return <span className={className}>{formatted}</span>
}
