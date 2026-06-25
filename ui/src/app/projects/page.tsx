import { redirect } from "next/navigation";

// The project picker now lives on Home (`/`). Keep this path working for old
// links and the breadcrumb by redirecting there.
export default function ProjectsPage() {
  redirect("/");
}
