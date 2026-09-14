declare module '@panzoom/panzoom/dist/panzoom.es.js' {
  import type { PanzoomGlobalOptions, PanzoomObject } from '@panzoom/panzoom';

  const Panzoom: (
    element: HTMLElement | SVGElement,
    options?: PanzoomGlobalOptions,
  ) => PanzoomObject;

  export default Panzoom;
}
