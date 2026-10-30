/** Tiny inline bandwidth sparkline. Scales to the series' own peak. */
export function Sparkline({
	values,
	width = 96,
	height = 22,
}: {
	values: number[]
	width?: number
	height?: number
}) {
	// With no (or all-zero) samples yet, still draw a flat baseline rather than
	// a dash, so the column reads as a quiet sparkline instead of empty.
	const hasData = values.some((v) => v > 0)
	const n = Math.max(values.length, 2)
	const max = Math.max(1, ...values)
	const stepX = width / (n - 1)
	const line = Array.from({ length: n }, (_, i) => {
		const v = values[i] ?? 0
		const x = i * stepX
		const y = height - 1 - (v / max) * (height - 2)
		return `${x.toFixed(1)},${y.toFixed(1)}`
	}).join(' ')

	return (
		<svg
			width={width}
			height={height}
			viewBox={`0 0 ${width} ${height}`}
			className={hasData ? 'text-primary' : 'text-muted-foreground/40'}
			aria-hidden
		>
			<polyline points={`0,${height} ${line} ${width},${height}`} fill="currentColor" fillOpacity={0.12} />
			<polyline
				points={line}
				fill="none"
				stroke="currentColor"
				strokeWidth={1.5}
				strokeLinejoin="round"
				strokeLinecap="round"
			/>
		</svg>
	)
}
