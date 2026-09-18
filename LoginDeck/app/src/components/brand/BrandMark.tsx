export type BrandMarkProps = { className?: string; title?: string };
const brandUrl = new URL('../../assets/logindeck.svg', import.meta.url).href;
export function BrandMark({ className, title }: BrandMarkProps) {
  return <img className={className} src={brandUrl} alt={title ?? ''} aria-hidden={title ? undefined : true} />;
}
