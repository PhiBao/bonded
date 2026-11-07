"use client";

import { useParams } from "next/navigation";
import JobDetail from "@/components/JobDetail";

export default function JobPage() {
  const params = useParams<{ id: string }>();
  return <JobDetail id={params.id} />;
}
