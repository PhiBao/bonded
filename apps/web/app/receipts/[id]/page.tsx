"use client";

import { useParams } from "next/navigation";
import JobDetail from "@/components/JobDetail";

export default function ReceiptPage() {
  const params = useParams<{ id: string }>();
  return <JobDetail id={params.id} receipt />;
}
