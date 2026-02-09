<script lang="ts">
	import { TimelineView } from '@opensession/ui/components';
	import type { Event } from '$lib/types';

	let { events }: { events: Event[] } = $props();
</script>

<TimelineView {events} />
