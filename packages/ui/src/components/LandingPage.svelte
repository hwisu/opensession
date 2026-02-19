<script lang="ts">
const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

const highlights = [
	'Capture AI sessions in open HAIL JSONL format',
	'Search and review sessions with timeline context',
	'Keep team workflow git-native without mandatory Docker ops',
];
</script>

<svelte:head>
	<title>OpenSession - Capture and Share AI Sessions</title>
</svelte:head>

<div class="mx-auto w-full max-w-5xl px-3 py-8 sm:px-6 sm:py-12">
	<section class="grid gap-6 lg:grid-cols-2 lg:gap-10">
		<div class="space-y-4">
			<p class="text-[11px] uppercase tracking-[0.18em] text-text-muted">open format • git native</p>
			<h1 class="text-3xl font-bold leading-tight text-text-primary sm:text-4xl">
				AI sessions are knowledge.
				<span class="block text-accent">Keep them searchable.</span>
			</h1>
			<p class="max-w-xl text-sm leading-relaxed text-text-secondary">
				OpenSession stores coding sessions as HAIL traces and gives your team a structured way
				to review what happened, what changed, and what to reuse.
			</p>
			<div class="flex flex-wrap gap-2">
				<button
					type="button"
					onclick={() => onNavigate('/login')}
					class="bg-accent px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-accent/85"
				>
					Sign In
				</button>
				<button
					type="button"
					onclick={() => onNavigate('/register')}
					class="border border-border px-4 py-2 text-xs font-semibold text-text-secondary transition-colors hover:border-accent hover:text-accent"
				>
					Create Account
				</button>
				<a
					href="https://github.com/hwisu/opensession"
					target="_blank"
					rel="noopener"
					class="px-3 py-2 text-xs text-text-muted underline-offset-2 transition-colors hover:text-text-primary hover:underline"
				>
					GitHub
				</a>
			</div>
		</div>

		<div class="border border-border bg-bg-secondary p-4 sm:p-5">
			<p class="mb-3 text-xs text-text-muted">Why teams use OpenSession</p>
			<ul class="space-y-2">
				{#each highlights as item}
					<li class="flex gap-2 text-xs text-text-secondary sm:text-sm">
						<span class="mt-[6px] h-1.5 w-1.5 shrink-0 rounded-full bg-accent"></span>
						<span>{item}</span>
					</li>
				{/each}
			</ul>
			<div class="mt-4 border-t border-border pt-3 text-[11px] text-text-muted">
				Browse docs without login. Sign in to upload and manage private workflows.
			</div>
		</div>
	</section>
</div>
