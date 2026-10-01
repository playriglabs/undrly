// Shared Tailwind class strings for patterns used across several components.
export const ui = {
  shell:
    'mx-auto w-[min(1248px,calc(100%-112px))] max-[1100px]:w-[calc(100%-72px)] max-md:w-[calc(100%-40px)]',
  section: 'py-[132px] max-md:py-20',
  sectionHeading: 'mb-[57px] max-md:mb-[35px]',
  headingRow:
    'flex items-end justify-between gap-16 max-[1100px]:gap-10 max-md:flex-col max-md:items-start max-md:gap-[25px]',
  h2: 'text-[clamp(36px,4vw,54px)] leading-[1.13] tracking-[-0.035em] max-md:text-[38px] [&_span]:text-[#929b90]',
  sectionCopy:
    'max-w-[360px] pb-[3px] text-base leading-[1.75] text-muted max-[1100px]:max-w-[320px] max-md:max-w-[440px] max-md:text-[15px]',
  button:
    'group inline-flex items-center justify-center gap-6 border border-[#dbe4d3] bg-[#dbe4d3] px-[21px] py-3.5 text-[15px] font-[550] text-[#1a2317] transition-[background,box-shadow] duration-[180ms] hover:bg-[#eff5e9] hover:shadow-[0_3px_20px_#bedaa01a]',
  buttonSmall:
    'group inline-flex items-center justify-center gap-[18px] border border-[#dbe4d3] bg-[#dbe4d3] px-3.5 py-[9px] text-sm font-[550] text-[#1a2317] transition-[background,box-shadow] duration-[180ms] hover:bg-[#eff5e9] hover:shadow-[0_3px_20px_#bedaa01a]',
  buttonGhost:
    'group inline-flex items-center justify-center gap-6 border border-transparent px-[21px] py-3.5 text-[15px] font-[550] text-ink transition-colors duration-[180ms] hover:bg-card',
  textLink: 'group inline-flex items-center gap-4 text-sm font-medium',
};
