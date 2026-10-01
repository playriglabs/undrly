import {
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  createChart,
  type IChartApi,
  LineSeries,
  LineStyle,
  type UTCTimestamp,
} from "lightweight-charts";
import { useEffect, useRef } from "react";
import type { SeriesBar } from "../lib/api";

export type ChartMode = "candles" | "line";

// Brand tokens (src/styles.css); the canvas cannot read CSS variables.
const COLORS = {
  surface: "#0e110f",
  grid: "#171c18",
  border: "#242925",
  text: "#6f776e",
  crosshair: "#3a4436",
  up: "#cfeea0",
  down: "#e3876b",
};

/** Decimals shown on the price scale: as many as the data carries, at most 8. */
function precisionOf(bars: SeriesBar[]) {
  let digits = 2;
  for (const b of bars) digits = Math.max(digits, b.close.split(".")[1]?.length ?? 0);
  return Math.min(digits, 8);
}

const seconds = (iso: string) => Math.floor(Date.parse(iso) / 1000) as UTCTimestamp;

/**
 * TradingView Lightweight Charts (Apache-2.0; its attribution logo stays on,
 * as the licence asks). Candles when the market has OHLC bars, a line
 * otherwise or on request. Browser-only: the chart is built after mount.
 * Exact decimal strings become floats here, for plotting only.
 */
export function TradingChart({
  bars,
  mode,
  intraday,
}: {
  bars: SeriesBar[];
  mode: ChartMode;
  intraday: boolean;
}) {
  const container = useRef<HTMLDivElement>(null);
  const chart = useRef<IChartApi | null>(null);

  useEffect(() => {
    const el = container.current;
    if (!el) return;
    const c = createChart(el, {
      autoSize: true,
      layout: {
        background: { type: ColorType.Solid, color: COLORS.surface },
        textColor: COLORS.text,
        fontFamily: "Pilat, sans-serif",
        fontSize: 12,
        attributionLogo: true,
      },
      grid: {
        vertLines: { color: COLORS.grid },
        horzLines: { color: COLORS.grid },
      },
      rightPriceScale: { borderColor: COLORS.border, scaleMargins: { top: 0.12, bottom: 0.08 } },
      timeScale: { borderColor: COLORS.border, timeVisible: intraday, secondsVisible: false },
      crosshair: {
        mode: CrosshairMode.Normal,
        vertLine: {
          color: COLORS.crosshair,
          style: LineStyle.Dashed,
          labelBackgroundColor: "#2c352a",
        },
        horzLine: {
          color: COLORS.crosshair,
          style: LineStyle.Dashed,
          labelBackgroundColor: "#2c352a",
        },
      },
      localization: { locale: "en-US" },
    });
    chart.current = c;

    const precision = precisionOf(bars);
    const priceFormat = { type: "price" as const, precision, minMove: 10 ** -precision };
    const hasOhlc = bars.every((b) => b.open && b.high && b.low);

    if (mode === "candles" && hasOhlc) {
      const series = c.addSeries(CandlestickSeries, {
        upColor: COLORS.up,
        downColor: COLORS.down,
        borderUpColor: COLORS.up,
        borderDownColor: COLORS.down,
        wickUpColor: COLORS.up,
        wickDownColor: COLORS.down,
        priceFormat,
      });
      series.setData(
        bars.map((b) => ({
          time: seconds(b.time),
          open: Number(b.open),
          high: Number(b.high),
          low: Number(b.low),
          close: Number(b.close),
        })),
      );
    } else {
      const up = Number(bars.at(-1)?.close) >= Number(bars[0]?.close);
      const series = c.addSeries(LineSeries, {
        color: up ? COLORS.up : COLORS.down,
        lineWidth: 2,
        priceFormat,
        crosshairMarkerBorderColor: COLORS.surface,
      });
      series.setData(bars.map((b) => ({ time: seconds(b.time), value: Number(b.close) })));
    }
    c.timeScale().fitContent();

    return () => {
      c.remove();
      chart.current = null;
    };
  }, [bars, mode, intraday]);

  return <div ref={container} className="h-[380px] w-full max-md:h-[260px]" />;
}
