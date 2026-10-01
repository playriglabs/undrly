import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { match } from 'ts-pattern';

/** Scoped animations; complete teardown supports navigation and hot reloads. */
export function animateLanding(root: HTMLElement) {
  gsap.registerPlugin(ScrollTrigger);
  const media = gsap.matchMedia();
  media.add('(prefers-reduced-motion: no-preference)', () => {
    const context = gsap.context(() => {
      gsap.from('[data-hero-reveal]', {
        y: 18,
        opacity: 0,
        duration: 0.85,
        stagger: 0.12,
        ease: 'power2.out',
      });
      gsap.utils.toArray<HTMLElement>('.reveal').forEach((element) => {
        gsap.from(element, {
          y: 18,
          opacity: 0,
          duration: 0.7,
          ease: 'power2.out',
          scrollTrigger: { trigger: element, start: 'top 94%', once: true },
        });
      });
      gsap.utils.toArray<HTMLElement>('[data-ornament]').forEach((element) => {
        const loop = gsap.timeline({ paused: true, repeat: -1, repeatDelay: 1.2, yoyo: true });
        match(element.dataset.ornament)
          .with('capture', () =>
            loop.to(element.querySelectorAll('.motion-source'), {
              y: '+=14',
              duration: 2.8,
              stagger: 0.3,
              ease: 'sine.inOut',
            }),
          )
          .with('modules', () =>
            loop.to(element.querySelectorAll('.motion-module'), {
              y: '-=12',
              duration: 2.4,
              stagger: 0.28,
              ease: 'sine.inOut',
            }),
          )
          .with('records', () =>
            loop.to(element.querySelectorAll('.motion-record'), {
              y: '-=10',
              x: '+=4',
              duration: 2.2,
              stagger: 0.09,
              ease: 'sine.inOut',
            }),
          )
          .otherwise(() => undefined);
        ScrollTrigger.create({
          trigger: element,
          start: 'top bottom',
          end: 'bottom top',
          onToggle: ({ isActive }) => {
            match(isActive)
              .with(true, () => loop.play())
              .with(false, () => loop.pause())
              .exhaustive();
          },
        });
      });
    }, root);
    return () => context.revert();
  });
  return () => media.revert();
}
