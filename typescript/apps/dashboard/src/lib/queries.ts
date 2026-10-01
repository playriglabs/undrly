import { keepPreviousData, queryOptions } from "@tanstack/react-query";
import {
  getMarketCounts,
  getMarketDetail,
  getMarkets,
  getSeries,
  type MarketsFilter,
  type Range,
} from "./api";

/** Prices move: the table and quote panels refresh while the page is open. */
const LIVE_MS = 15_000;

export const marketsQuery = (filter: MarketsFilter) =>
  queryOptions({
    queryKey: ["markets", filter.classes, filter.q, filter.page],
    queryFn: () => getMarkets({ data: filter }),
    placeholderData: keepPreviousData,
    refetchInterval: LIVE_MS,
  });

export const marketCountsQuery = () =>
  queryOptions({
    queryKey: ["market-counts"],
    queryFn: () => getMarketCounts(),
    staleTime: 60_000,
  });

export const marketDetailQuery = (id: string, unit: string | undefined) =>
  queryOptions({
    queryKey: ["market", id, unit ?? null],
    queryFn: () => getMarketDetail({ data: unit ? { id, unit } : { id } }),
    refetchInterval: LIVE_MS,
  });

export const seriesQuery = (id: string, unit: string | undefined, range: Range) =>
  queryOptions({
    queryKey: ["series", id, unit ?? null, range],
    queryFn: () => getSeries({ data: unit ? { id, unit, range } : { id, range } }),
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  });
