<script lang="ts">
	import { HomePage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { appProfile } from '$lib/profile';
</script>

<HomePage
	onNavigate={(path) => goto(path)}
	uploadEnabled={appProfile !== 'worker'}
	showLandingForGuests={appProfile === 'server'}
/>
