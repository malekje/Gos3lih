import { useMemo } from "react";

interface ThroughputGaugeProps {
  label: string;
  bitsPerSec: number;
  color: string;
}

function formatBits(bits: number): { value: string; unit: string } {
  if (bits >= 1_000_000_000) {
    return { value: (bits / 1_000_000_000).toFixed(2), unit: "Gbps" };
  }
  if (bits >= 1_000_000) {
    return { value: (bits / 1_000_000).toFixed(1), unit: "Mbps" };
  }
  if (bits >= 1_000) {
    return { value: (bits / 1_000).toFixed(0), unit: "Kbps" };
  }
  return { value: bits.toFixed(0), unit: "bps" };
}

export function ThroughputGauge({ label, bitsPerSec, color }: ThroughputGaugeProps) {
  const formatted = useMemo(() => formatBits(bitsPerSec), [bitsPerSec]);

  // SVG arc gauge (180°)
  const maxBps = 1_000_000_000; // 1 Gbps max scale
  const ratio = Math.min(bitsPerSec / maxBps, 1);
  const angle = ratio * 180;

  // Arc path calculation
  const cx = 120,
    cy = 110,
    r = 90;
  const startAngle = -180;
  const endAngle = startAngle + angle;

  const polarToCartesian = (a: number) => ({
    x: cx + r * Math.cos((a * Math.PI) / 180),
    y: cy + r * Math.sin((a * Math.PI) / 180),
  });

  const start = polarToCartesian(startAngle);
  const end = polarToCartesian(endAngle);
  const largeArc = angle > 180 ? 1 : 0;

  const arcPath =
    angle > 0
      ? `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} 1 ${end.x} ${end.y}`
      : "";

  // Background arc (full 180°)
  const bgEnd = polarToCartesian(0);
  const bgPath = `M ${start.x} ${start.y} A ${r} ${r} 0 1 1 ${bgEnd.x} ${bgEnd.y}`;

  return (
    <div className="bg-gray-900 rounded-2xl border border-gray-800 p-6 flex flex-col items-center">
      <svg width="240" height="130" viewBox="0 0 240 130">
        {/* Background track */}
        <path
          d={bgPath}
          fill="none"
          stroke="#1f2937"
          strokeWidth="14"
          strokeLinecap="round"
        />
        {/* Active arc */}
        {arcPath && (
          <path
            d={arcPath}
            fill="none"
            stroke={color}
            strokeWidth="14"
            strokeLinecap="round"
            style={{
              filter: `drop-shadow(0 0 8px ${color}60)`,
              transition: "d 0.5s ease-out",
            }}
          />
        )}
        {/* Value text */}
        <text
          x={cx}
          y={cy - 10}
          textAnchor="middle"
          className="fill-white text-3xl font-bold"
          style={{ fontSize: "32px", fontWeight: 700 }}
        >
          {formatted.value}
        </text>
        <text
          x={cx}
          y={cy + 15}
          textAnchor="middle"
          className="fill-gray-400 text-sm"
          style={{ fontSize: "14px" }}
        >
          {formatted.unit}
        </text>
      </svg>
      <span className="mt-2 text-sm font-medium text-gray-300">{label}</span>
    </div>
  );
}
