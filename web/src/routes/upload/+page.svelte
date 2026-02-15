<script lang="ts">
	import { UploadPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { appProfile } from '$lib/profile';
</script>

<UploadPage
	teamMode={appProfile === 'worker' ? 'personal' : 'manual'}
	onSuccess={(id) => goto(`/session/${id}`)}
	onNavigate={(path) => goto(path)}
/>
