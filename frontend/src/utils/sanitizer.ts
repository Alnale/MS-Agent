import DOMPurify from 'dompurify';

/**
 * Centralized DOMPurify configuration.
 * Import from this module instead of using DOMPurify directly to ensure consistent sanitization.
 */
export const sanitize = DOMPurify.sanitize.bind(DOMPurify);

export const sanitizeSvg = (html: string) =>
  DOMPurify.sanitize(html, { USE_PROFILES: { svg: true, svgFilters: true } });

export default DOMPurify;
