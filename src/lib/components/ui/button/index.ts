import Root from "./button.svelte";
import { tv, type VariantProps } from "tailwind-variants";
import type { HTMLButtonAttributes } from "svelte/elements";

const buttonVariants = tv({
	base: "inline-flex items-center justify-center rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:opacity-50 disabled:pointer-events-none ring-offset-background",
	variants: {
		variant: {
			default: "bg-zinc-100 text-zinc-900 hover:bg-zinc-200",
			destructive: "bg-red-600 text-white hover:bg-red-700",
			outline: "border border-zinc-800 bg-transparent hover:bg-zinc-900 hover:text-zinc-100",
			secondary: "bg-zinc-800 text-zinc-100 hover:bg-zinc-700",
			ghost: "hover:bg-zinc-900 hover:text-zinc-100",
			link: "underline-offset-4 hover:underline text-zinc-100",
		},
		size: {
			default: "h-10 py-2 px-4",
			sm: "h-9 px-3 rounded-md",
			lg: "h-11 px-8 rounded-md",
			icon: "h-10 w-10",
		},
	},
	defaultVariants: {
		variant: "default",
		size: "default",
	},
});

type Variant = VariantProps<typeof buttonVariants>["variant"];
type Size = VariantProps<typeof buttonVariants>["size"];

type Props = HTMLButtonAttributes & {
	variant?: Variant;
	size?: Size;
};

export {
	Root,
	type Props,
	buttonVariants,
	Root as Button,
};
