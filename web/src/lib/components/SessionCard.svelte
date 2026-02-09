<script lang="ts">
	import { SessionCard } from '@opensession/ui/components';
	import type { SessionListItem } from '$lib/types';

	let { session }: { session: SessionListItem } = $props();
</script>

<SessionCard {session} />
