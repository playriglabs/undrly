import { infiniteQueryOptions, keepPreviousData, queryOptions } from "@tanstack/react-query";
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

/**
 * The market table, a page of 50 at a time as it scrolls. Every loaded page
 * refreshes with the rest of the dashboard.
 */
export const marketsQuery = (filter: Omit<MarketsFilter, "page">) =>
  infiniteQueryOptions({
    queryKey: ["markets", filter.classes, filter.q, filter.sort],
    queryFn: ({ pageParam }) => getMarkets({ data: { ...filter, page: pageParam } }),
    initialPageParam: 1,
    getNextPageParam: (last, all) =>
      last.ok && last.data.offset + last.data.markets.length < last.data.total
        ? all.length + 1
        : undefined,
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
